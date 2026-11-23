#pragma once
#import "SFOwner.h"
#import "SFGoTransport.h"

typedef void (^SFClientCloseCompletion)(SfmStatus status);
typedef id (^SFClientDeferredWork)(SfmStatus *status);
typedef void (^SFClientDeferredDone)(id value, SfmStatus status);
typedef NS_ENUM(NSUInteger, SFClientAdmission) {
  SFClientAdmissionOrdinary,
  SFClientAdmissionV1FileRequest,
  SFClientAdmissionV1RetireView,
  SFClientAdmissionV1Reconnect,
};
@class SFClientDeferredCall;
@interface SFClientRecord : NSObject
@property(nonatomic) SfmClientHandle handle;
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
// Tells JS that read has results; at most once until read returns nil.
@property(nonatomic, copy) void (^eventSignal)(NSString *identifier);
@property(nonatomic) BOOL signalled;
@end

// All methods run on SFOwner, including terminal lease and reset coordination.
@interface SFClientStore : NSObject <SFRemoteTerminalSink>
+ (instancetype)shared;
// Returns YES when a state without its own wake needs another pass soon.
- (BOOL)tick;
- (SfmStatus)createPeer:(NSString *)peer clientId:(NSString *)clientId result:(NSString **)result;
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
- (void)whenDrained:(NSString *)identifier admission:(SFClientAdmission)admission
              work:(SFClientDeferredWork)work done:(SFClientDeferredDone)done;
- (void)drainClient:(SFClientRecord *)client;
- (void)retirePending:(SFClientRecord *)client;
- (void)clearFailedPollAfterV1Fence:(SFClientRecord *)client;
- (void)completed:(NSDictionary *)event client:(SFClientRecord *)client;
- (BOOL)shouldForward:(id)event client:(SFClientRecord *)client;
@end
