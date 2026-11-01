// A UI-only bridge: authorization never constructs an input or captures samples.
#import <AVFoundation/AVFoundation.h>
#import <Foundation/Foundation.h>
#include <stdbool.h>
#include <stdatomic.h>

static atomic_bool pending = false;

// 0 undetermined, 1 restricted, 2 denied, 3 allowed, 4 pending, 5 missing bundle description.
int orqwa_microphone_permission(bool request) {
    @autoreleasepool {
        AVAuthorizationStatus status = [AVCaptureDevice authorizationStatusForMediaType:AVMediaTypeAudio];
        switch (status) {
            case AVAuthorizationStatusRestricted: return 1;
            case AVAuthorizationStatusDenied: return 2;
            case AVAuthorizationStatusAuthorized: return 3;
            case AVAuthorizationStatusNotDetermined: break;
            default: return 1;
        }
        if (atomic_load(&pending)) {
            return 4;
        }
        if (!request) {
            return 0;
        }
        // A bare editor executable may lack this key; requesting would terminate the app.
        id description = [[NSBundle mainBundle] objectForInfoDictionaryKey:@"NSMicrophoneUsageDescription"];
        if (![description isKindOfClass:[NSString class]] || [description length] == 0) {
            return 5;
        }
        if (!atomic_exchange(&pending, true)) {
            [AVCaptureDevice requestAccessForMediaType:AVMediaTypeAudio completionHandler:^(BOOL granted) {
                (void)granted;
                atomic_store(&pending, false);
            }];
        }
        return 4;
    }
}
