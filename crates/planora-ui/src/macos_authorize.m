#import <Foundation/Foundation.h>
#import <ServiceManagement/ServiceManagement.h>

int planora_register_daemon(void) {
    if (@available(macOS 13.0, *)) {
        NSError *error = nil;
        SMAppService *service = [SMAppService daemonServiceWithPlistName:@"org.planora.helper.plist"];
        BOOL registered = [service registerAndReturnError:&error];
        if (registered) {
            [SMAppService openSystemSettingsLoginItems];
            return 0;
        }
        NSLog(@"Planora helper registration failed: %@", error);
        return 1;
    }
    return 2;
}

int planora_unregister_daemon(void) {
    if (@available(macOS 13.0, *)) {
        NSError *error = nil;
        BOOL removed = [[SMAppService daemonServiceWithPlistName:@"org.planora.helper.plist"] unregisterAndReturnError:&error];
        if (!removed) NSLog(@"Planora helper unregistration failed: %@", error);
        return removed ? 0 : 1;
    }
    return 2;
}
