#pragma once
#import "SFOwner.h"
#import "SFGoTransport.h"
#import "SFClientProfile.h"

typedef void (^SFClientCloseCompletion)(SfmStatus status);
typedef id (^SFClientDeferredWork)(SfmStatus *status);
typedef void (^SFClientDeferredDone)(id value, SfmStatus status);
@class SFClientDeferredCall;
@interface SFClientRecord : NSObject
@property(nonatomic) SfmClientHandle handle;
@property(nonatomic) uint32_t profile;
@property(nonatomic) uint32_t apiCapabilities;
- (const shellfox::ClientAPI &)functions;
@property(nonatomic, strong) NSMutableArray<NSData *> *events;
@property(nonatomic) NSUInteger eventBytes;
@property(nonatomic, strong) NSMutableSet<NSString *> *resets;
@property(nonatomic, strong) NSMutableSet<NSString *> *explicitOperations;
@property(nonatomic, copy) NSString *pollOperation;
@property(nonatomic, copy) NSString *connectionKey;
@property(nonatomic, strong) NSMutableDictionary<NSString *, NSString *> *resetTickets;
@property(nonatomic) BOOL polling;
@property(nonatomic) NSTimeInterval nextPoll;
@property(nonatomic) BOOL closing;
@property(nonatomic, strong) NSMutableArray<SFClientDeferredCall *> *pendingCalls;
@property(nonatomic, strong) NSMutableSet<NSString *> *mutations;
@property(nonatomic, copy) NSString *authorityKey;
@property(nonatomic) NSUInteger authorityEpoch;
@property(nonatomic) SfmStatus pollFailure;
@property(nonatomic) BOOL nativeCloseAttempted;
@property(nonatomic) SfmStatus closeStatus;
@property(nonatomic) BOOL disconnectFinished;
@property(nonatomic, copy) NSString *disconnectOperation;
@property(nonatomic) NSTimeInterval closeDeadline;
@property(nonatomic, strong) NSMutableArray<SFClientCloseCompletion> *closeWaiters;
@end

// All methods run on SFOwner, including terminal lease and reset coordination.
@interface SFClientStore : NSObject <SFRemoteTerminalSink>
+ (instancetype)shared;
- (void)tick;
- (SfmStatus)createPeer:(NSString *)peer clientId:(NSString *)clientId result:(NSString **)result;
- (SfmStatus)createObserverPeer:(NSString *)peer clientId:(NSString *)clientId result:(NSString **)result;
- (SfmStatus)createManagerPeer:(NSString *)peer clientId:(NSString *)clientId result:(NSString **)result;
- (SFClientRecord *)client:(NSString *)identifier;
- (SfmStatus)close:(NSString *)identifier;
- (SfmStatus)beginClose:(NSString *)identifier completion:(SFClientCloseCompletion)completion;
- (SfmStatus)bind:(NSString *)identifier tab:(NSString *)tab terminal:(NSString *)terminal;
- (SfmStatus)unbind:(NSString *)identifier terminal:(NSString *)terminal;
- (NSData *)read:(NSString *)identifier;
- (BOOL)hasPendingIntents:(NSString *)identifier;
- (BOOL)isBound:(NSString *)terminal client:(SFClientRecord *)client;
- (BOOL)recoveringViews:(SFClientRecord *)client;
- (BOOL)ownerDrained:(SFClientRecord *)client;
@end
@interface SFClientStore (Polling)
- (SfmStatus)poll:(NSString *)identifier operation:(uint64_t *)operation;
- (void)whenDrained:(NSString *)identifier work:(SFClientDeferredWork)work done:(SFClientDeferredDone)done;
- (void)drainClient:(SFClientRecord *)client;
- (void)retirePending:(SFClientRecord *)client;
- (void)completed:(NSDictionary *)event client:(SFClientRecord *)client;
- (BOOL)shouldForward:(id)event client:(SFClientRecord *)client;
@end
