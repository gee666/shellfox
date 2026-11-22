#pragma once
#import <Foundation/Foundation.h>
#import "SFFrame.h"
#import "SFIntentQueue.h"

NS_ASSUME_NONNULL_BEGIN
BOOL SFParseId(NSString *value, uint64_t *result);
NSString *SFDecimal(uint64_t value);
// Implemented by the Rust client adapter, not by JavaScript. Calls happen on owner.
@protocol SFRemoteTerminalSink <NSObject>
- (SfmStatus)sendInput:(NSData *)bytes userInitiated:(uint8_t)hint terminal:(NSString *)terminal;
- (SfmStatus)sendCanonicalInput:(NSData *)json terminal:(NSString *)terminal;
- (SfmStatus)resizeTerminal:(NSString *)terminal columns:(uint16_t)columns rows:(uint16_t)rows;
- (SfmStatus)consumedTerminal:(NSString *)terminal sequence:(uint64_t)sequence;
- (SfmStatus)recoveryReady:(NSString *)terminal ready:(uint8_t *)ready;
- (void)terminalFailed:(NSString *)terminal status:(SfmStatus)status;
@end

@interface SFTerminal : NSObject
@property(nonatomic, copy, readonly) NSString *identifier;
@property(nonatomic, readonly) SfmHandle handle;
@property(nonatomic, weak, nullable) id<SFRemoteTerminalSink> sink;
@property(nonatomic) BOOL visible;
@property(nonatomic) BOOL retiring;
@property(nonatomic) BOOL closeAttempted;
@property(nonatomic) BOOL dirty;
@property(nonatomic) uint64_t delivered;
@property(nonatomic) uint16_t columns;
@property(nonatomic) uint16_t rows;
@property(nonatomic) uint16_t viewportColumns;
@property(nonatomic) uint16_t viewportRows;
@property(nonatomic) BOOL readOnly;
@property(nonatomic) BOOL deliveryOnly;
@property(nonatomic) BOOL hostEncoding;
@property(nonatomic) BOOL recovering;
@property(nonatomic) BOOL recoveryInstalled;
@property(nonatomic) BOOL viewportUnaccepted;
@property(nonatomic, strong) SFIntentQueue *pendingIntents;
@property(nonatomic) std::shared_ptr<shellfox::Mailbox> mailbox;
@end

@interface SFLifetime : NSObject {
@public
  std::atomic<bool> invalidated;
}
@property(nonatomic, strong) NSMutableSet<NSString *> *terminals;
@property(nonatomic, strong) NSMutableSet<NSString *> *clients;
@end

typedef void (^SFOwnerWork)(void);
@interface SFOwner : NSObject
+ (instancetype)shared;
// Bounded admission. Accepted blocks execute in FIFO order on one permanent NSThread.
- (BOOL)enqueue:(SFOwnerWork)work;
// Any thread. Runs a client and terminal pass as soon as the owner is free.
- (void)requestTick;
- (BOOL)isOwnerThread;
- (BOOL)registerLifetime:(SFLifetime *)lifetime;
- (SfmStatus)create:(uint16_t)cols rows:(uint16_t)rows result:(SFTerminal *_Nullable *_Nonnull)result;
- (nullable SFTerminal *)terminal:(NSString *)identifier;
- (SfmStatus)close:(NSString *)identifier;
@end

@interface SFOwner (Input)
- (SfmStatus)resize:(NSString *)identifier columns:(uint16_t)cols rows:(uint16_t)rows;
- (SfmStatus)input:(NSString *)identifier event:(NSDictionary *)event;
- (SfmStatus)input:(NSString *)identifier events:(NSArray<NSDictionary *> *)events;
@end
NS_ASSUME_NONNULL_END
