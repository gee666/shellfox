#pragma once
#import <Foundation/Foundation.h>
#include "shellfox_mobile.h"

typedef NS_ENUM(uint8_t, SFIntentKind) { SFIntentWrite, SFIntentRelease, SFIntentResize };
@interface SFNativeIntent : NSObject
@property(nonatomic) SFIntentKind kind;
// Copied from SfmInputExFn. Admission priority is separate from this Rust hint.
@property(nonatomic) uint8_t userInitiated;
@property(nonatomic, strong, nullable) NSData *data;
@property(nonatomic) uint16_t columns;
@property(nonatomic) uint16_t rows;
@end

// Owner-thread preadmission only. Accepted Rust operations never enter this queue.
@interface SFIntentQueue : NSObject
@property(nonatomic, readonly) NSUInteger count;
@property(nonatomic, readonly) NSUInteger bytes;
- (nullable SFNativeIntent *)front;
- (SfmStatus)append:(SFNativeIntent *)intent;
- (SfmStatus)appendBatch:(NSArray<SFNativeIntent *> *)intents;
- (void)pop;
- (void)clear;
@end
