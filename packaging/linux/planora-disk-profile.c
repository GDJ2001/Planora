#define _GNU_SOURCE
// Private, fixed-purpose libbpf collector. The helper reads this bounded
// binary stream from a pipe; this executable has no user-selected BPF object,
// path, program, or tracing-session name.
#include <bpf/bpf.h>
#include <bpf/libbpf.h>
#include <errno.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <sys/time.h>
#include <unistd.h>

#ifndef PLANORA_BPF_OBJECT
#define PLANORA_BPF_OBJECT "/usr/libexec/planora/planora-disk-profile.bpf.o"
#endif
#define PLANORA_PROFILE_SOCKET "/run/planora-profile/planora-profile.sock"
#define MAX_EVENTS 1400000

struct disk_event {
    uint64_t issue_ns;
    uint64_t complete_ns;
    uint64_t process_start_ns;
    uint64_t thread_start_ns;
    uint64_t bytes;
    uint32_t pid;
    uint32_t tid;
    uint32_t direction;
    uint32_t request_complete;
    char comm[16];
};

_Static_assert(sizeof(struct disk_event) == 72, "BPF event ABI changed");

struct stream_state {
    uint64_t events;
    int write_failed;
    int output_fd;
};

static int write_all(int fd, const void *data, size_t size)
{
    const unsigned char *cursor = data;
    while (size) {
        ssize_t count = write(fd, cursor, size);
        if (count < 0 && errno == EINTR)
            continue;
        if (count <= 0)
            return -1;
        cursor += count;
        size -= (size_t)count;
    }
    return 0;
}

static int on_event(void *context, void *data, size_t size)
{
    struct stream_state *state = context;
    if (size != sizeof(struct disk_event) || state->events >= MAX_EVENTS) {
        state->write_failed = 1;
        return -1;
    }
    const unsigned char kind = 1;
    if (write_all(state->output_fd, &kind, 1) ||
        write_all(state->output_fd, data, size)) {
        state->write_failed = 1;
        return -1;
    }
    state->events++;
    return 0;
}

static uint64_t monotonic_ms(void)
{
    struct timespec stamp;
    if (clock_gettime(CLOCK_MONOTONIC, &stamp))
        return 0;
    return (uint64_t)stamp.tv_sec * 1000 + (uint64_t)stamp.tv_nsec / 1000000;
}

static int run_collection(int duration, int control_fd, int output_fd)
{
    struct stat object_stat;
    if (lstat(PLANORA_BPF_OBJECT, &object_stat) ||
        !S_ISREG(object_stat.st_mode) || object_stat.st_uid != 0 ||
        (object_stat.st_mode & 022) != 0) {
        fputs("Planora BPF object is missing or not administrator-owned\n", stderr);
        return 3;
    }
    struct bpf_object *object = bpf_object__open_file(PLANORA_BPF_OBJECT, NULL);
    if (!object || libbpf_get_error(object)) {
        fputs("Planora BPF object could not be opened\n", stderr);
        return 4;
    }
    if (bpf_object__load(object)) {
        fputs("Planora BPF object could not be loaded; check kernel BTF and service capabilities\n", stderr);
        bpf_object__close(object);
        return 5;
    }
    struct stat namespace_stat;
    struct bpf_map *namespace_map = bpf_object__find_map_by_name(object, "pid_namespace");
    uint32_t namespace_key = 0;
    struct {
        uint64_t device;
        uint64_t inode;
    } namespace_identity;
    if (!namespace_map || stat("/proc/self/ns/pid", &namespace_stat)) {
        fputs("Planora could not identify its Linux PID namespace\n", stderr);
        bpf_object__close(object);
        return 5;
    }
    namespace_identity.device = namespace_stat.st_dev;
    namespace_identity.inode = namespace_stat.st_ino;
    if (bpf_map_update_elem(bpf_map__fd(namespace_map), &namespace_key,
                            &namespace_identity, BPF_ANY)) {
        fputs("Planora could not configure its Linux PID namespace\n", stderr);
        bpf_object__close(object);
        return 5;
    }
    struct bpf_link *links[2] = {NULL, NULL};
    struct bpf_program *program;
    size_t attached = 0;
    bpf_object__for_each_program(program, object) {
        if (attached >= 2)
            break;
        struct bpf_link *link = bpf_program__attach(program);
        if (!link || libbpf_get_error(link)) {
            fputs("Planora block tracing hooks are unavailable\n", stderr);
            goto fail;
        }
        links[attached++] = link;
    }
    if (attached != 2) {
        fputs("Planora BPF object has an unexpected program count\n", stderr);
        goto fail;
    }
    if (duration == 0) {
        const unsigned char ready = 0;
        int output_ok = !write_all(output_fd, &ready, 1);
        bpf_link__destroy(links[1]);
        bpf_link__destroy(links[0]);
        bpf_object__close(object);
        return output_ok ? 0 : 6;
    }

    struct bpf_map *events = bpf_object__find_map_by_name(object, "events");
    struct bpf_map *losses = bpf_object__find_map_by_name(object, "losses");
    if (!events || !losses)
        goto fail;
    struct stream_state state = {.output_fd = output_fd};
    struct ring_buffer *ring = ring_buffer__new(bpf_map__fd(events), on_event, &state, NULL);
    if (!ring || libbpf_get_error(ring))
        goto fail;

    const uint64_t started = monotonic_ms();
    int cancelled = 0;
    while (monotonic_ms() - started < (uint64_t)duration * 1000) {
        if (ring_buffer__poll(ring, 100) < 0 || state.write_failed) {
            ring_buffer__free(ring);
            goto fail;
        }
        struct pollfd control = {.fd = control_fd, .events = POLLIN | POLLHUP};
        if (poll(&control, 1, 0) > 0) {
            cancelled = 1;
            break;
        }
    }
    uint32_t key = 0;
    uint64_t lost = 0;
    (void)bpf_map_lookup_elem(bpf_map__fd(losses), &key, &lost);
    const uint64_t elapsed = monotonic_ms() - started;
    const unsigned char footer_kind = 2;
    const unsigned char was_cancelled = (unsigned char)cancelled;
    int output_ok = !write_all(output_fd, &footer_kind, 1) &&
                    !write_all(output_fd, &lost, sizeof(lost)) &&
                    !write_all(output_fd, &elapsed, sizeof(elapsed)) &&
                    !write_all(output_fd, &was_cancelled, 1);
    ring_buffer__free(ring);
    bpf_link__destroy(links[1]);
    bpf_link__destroy(links[0]);
    bpf_object__close(object);
    return output_ok ? 0 : 6;

fail:
    for (size_t i = 0; i < attached; i++)
        bpf_link__destroy(links[i]);
    bpf_object__close(object);
    return 7;
}

static int serve(void)
{
    int listener = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (listener < 0)
        return 8;
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    if (strlen(PLANORA_PROFILE_SOCKET) >= sizeof(address.sun_path))
        return 8;
    strcpy(address.sun_path, PLANORA_PROFILE_SOCKET);
    unlink(PLANORA_PROFILE_SOCKET);
    umask(0077);
    if (bind(listener, (struct sockaddr *)&address, sizeof(address)) ||
        chmod(PLANORA_PROFILE_SOCKET, 0600) || listen(listener, 1)) {
        close(listener);
        return 8;
    }
    for (;;) {
        int client = accept4(listener, NULL, NULL, SOCK_CLOEXEC);
        if (client < 0) {
            if (errno == EINTR)
                continue;
            break;
        }
        struct ucred peer;
        socklen_t peer_size = sizeof(peer);
        if (getsockopt(client, SOL_SOCKET, SO_PEERCRED, &peer, &peer_size) ||
            peer_size != sizeof(peer) || peer.uid != geteuid()) {
            close(client);
            continue;
        }
        const struct timeval deadline = {.tv_sec = 2};
        (void)setsockopt(client, SOL_SOCKET, SO_RCVTIMEO, &deadline, sizeof(deadline));
        (void)setsockopt(client, SOL_SOCKET, SO_SNDTIMEO, &deadline, sizeof(deadline));
        uint32_t duration = 0;
        size_t read_bytes = 0;
        while (read_bytes < sizeof(duration)) {
            ssize_t count = read(client, (char *)&duration + read_bytes,
                                 sizeof(duration) - read_bytes);
            if (count <= 0)
                break;
            read_bytes += (size_t)count;
        }
        if (read_bytes == sizeof(duration) && duration <= 60)
            (void)run_collection((int)duration, client, client);
        close(client);
    }
    close(listener);
    unlink(PLANORA_PROFILE_SOCKET);
    return 8;
}

int main(int argc, char **argv)
{
    // A disconnected helper cancels its own profile. Never let a broken
    // stream terminate the long-lived profiling service via SIGPIPE.
    signal(SIGPIPE, SIG_IGN);
    if (argc == 2 && !strcmp(argv[1], "--serve"))
        return serve();
    if (argc == 2 && !strcmp(argv[1], "--probe"))
        return run_collection(0, STDIN_FILENO, STDOUT_FILENO);
    if (argc == 3 && !strcmp(argv[1], "--run")) {
        char *end = NULL;
        long parsed = strtol(argv[2], &end, 10);
        if (!end || *end || parsed < 1 || parsed > 60)
            return 2;
        return run_collection((int)parsed, STDIN_FILENO, STDOUT_FILENO);
    }
    return 2;
}
