// SPDX-License-Identifier: MIT OR GPL-2.0-only
// Fixed read-only block request collector. Build with clang -target bpf and
// libbpf headers; CO-RE relocations resolve the request and task fields.
// The object accepts no user-provided code, paths, or kernel addresses.
#include <linux/bpf.h>
#include <bpf/bpf_helpers.h>
#include <bpf/bpf_core_read.h>

typedef unsigned long long u64;
typedef unsigned int u32;

struct request {
    u32 cmd_flags;
    u32 __data_len;
} __attribute__((preserve_access_index));

struct task_struct {
    struct task_struct *group_leader;
    u64 start_boottime;
    u32 flags;
} __attribute__((preserve_access_index));

struct pending_request {
    u64 issue_ns;
    u64 process_start_ns;
    u64 thread_start_ns;
    u64 remaining;
    u32 pid;
    u32 tid;
    u32 direction;
    char comm[16];
};

struct disk_event {
    u64 issue_ns;
    u64 complete_ns;
    u64 process_start_ns;
    u64 thread_start_ns;
    u64 bytes;
    u32 pid;
    u32 tid;
    u32 direction;
    u32 request_complete;
    char comm[16];
};

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 65536);
    __type(key, u64);
    __type(value, struct pending_request);
} pending SEC(".maps");

struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 8 * 1024 * 1024);
} events SEC(".maps");

struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, u64);
} losses SEC(".maps");

struct pid_namespace_identity {
    u64 device;
    u64 inode;
};

struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, struct pid_namespace_identity);
} pid_namespace SEC(".maps");

static __always_inline void count_loss(void)
{
    u32 key = 0;
    u64 *counter = bpf_map_lookup_elem(&losses, &key);
    if (counter)
        __sync_fetch_and_add(counter, 1);
}

SEC("raw_tracepoint/block_rq_issue")
int planora_block_issue(struct bpf_raw_tracepoint_args *ctx)
{
    struct request *rq = (void *)ctx->args[0];
    u32 op = BPF_CORE_READ(rq, cmd_flags) & 0xff;
    if (op != 0 && op != 1)
        return 0; // reads and writes only; discard/flush are different units

    u64 size = BPF_CORE_READ(rq, __data_len);
    if (!size)
        return 0;
    u32 namespace_key = 0;
    struct pid_namespace_identity *ns = bpf_map_lookup_elem(&pid_namespace, &namespace_key);
    struct bpf_pidns_info namespace_pid = {};
    int has_namespace_pid = ns &&
        !bpf_get_ns_current_pid_tgid(ns->device, ns->inode,
                                     &namespace_pid, sizeof(namespace_pid));
    struct task_struct *task = (void *)bpf_get_current_task_btf();
    struct task_struct *leader = BPF_CORE_READ(task, group_leader);
    struct pending_request request = {};
    request.issue_ns = bpf_ktime_get_ns();
    request.process_start_ns = leader ? BPF_CORE_READ(leader, start_boottime) : 0;
    request.thread_start_ns = BPF_CORE_READ(task, start_boottime);
    request.remaining = size;
    // Kernel writeback/completion work has no verified user-process owner.
    // PF_KTHREAD is the kernel task flag; keep those bytes unattributed.
    if (has_namespace_pid && !(BPF_CORE_READ(task, flags) & 0x00200000)) {
        request.pid = namespace_pid.tgid;
        request.tid = namespace_pid.pid;
    }
    request.direction = op;
    bpf_get_current_comm(&request.comm, sizeof(request.comm));
    u64 key = (u64)rq;
    if (bpf_map_update_elem(&pending, &key, &request, BPF_ANY))
        count_loss();
    return 0;
}

SEC("raw_tracepoint/block_rq_complete")
int planora_block_complete(struct bpf_raw_tracepoint_args *ctx)
{
    struct request *rq = (void *)ctx->args[0];
    u64 key = (u64)rq;
    struct pending_request *request = bpf_map_lookup_elem(&pending, &key);
    if (!request) {
        u32 op = BPF_CORE_READ(rq, cmd_flags) & 0xff;
        if (op == 0 || op == 1)
            count_loss(); // issued before tracing or dropped when map was full
        return 0;
    }
    // The third raw tracepoint argument is the number of bytes completed.
    u64 bytes = ctx->args[2];
    if (!bytes || bytes > request->remaining)
        bytes = request->remaining;
    if (!bytes)
        return 0;

    struct disk_event *event = bpf_ringbuf_reserve(&events, sizeof(*event), 0);
    if (!event) {
        count_loss();
    } else {
        event->issue_ns = request->issue_ns;
        event->complete_ns = bpf_ktime_get_ns();
        event->process_start_ns = request->process_start_ns;
        event->thread_start_ns = request->thread_start_ns;
        event->bytes = bytes;
        event->pid = request->pid;
        event->tid = request->tid;
        event->direction = request->direction;
        event->request_complete = bytes >= request->remaining;
        __builtin_memcpy(event->comm, request->comm, sizeof(event->comm));
        bpf_ringbuf_submit(event, 0);
    }
    request->remaining -= bytes;
    if (!request->remaining)
        bpf_map_delete_elem(&pending, &key);
    return 0;
}

char LICENSE[] SEC("license") = "Dual MIT/GPL";
