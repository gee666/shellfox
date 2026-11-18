#pragma once
#import <Foundation/Foundation.h>
#include "shellfox_mobile_client.h"

typedef NSString *_Nullable (^SFGoRequestFunction)(NSString *peer, NSString *path, NSString *body,
                                                    NSError *_Nullable *_Nullable error);
@interface SFGoHTTPResponse : NSObject
@property(nonatomic, readonly) long statusCode;
@property(nonatomic, copy, readonly) NSString *body;
- (instancetype)initWithStatusCode:(long)status body:(NSString *)body;
@end

typedef SFGoHTTPResponse *_Nullable (^SFGoStatusRequestFunction)(NSString *peer, NSString *path, NSString *body,
                                                               NSError *_Nullable *_Nullable error);
@interface SFGoTransport : NSObject
+ (instancetype)shared;
// Native fixture injection only. Production initializers use immutable Go results.
- (instancetype)initWithRequestFunction:(SFGoRequestFunction)request;
- (instancetype)initWithRequestFunction:(SFGoRequestFunction)request statusRequestFunction:(SFGoStatusRequestFunction)statusRequest;
- (void)dispatchClient:(SfmClientHandle)client;
- (SfmStatus)closeClient:(SfmClientHandle)client;
@end
