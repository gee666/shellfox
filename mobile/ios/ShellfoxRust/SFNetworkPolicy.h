#pragma once
#import <Foundation/Foundation.h>

// Exact carrier paths for the native Rust transport. Go checks them again.
static inline BOOL SFNetworkPathAllowed(NSString *path) {
  if (![path isKindOfClass:NSString.class]) return NO;
  return [@[@"/v1/discover", @"/v1/pair", @"/v1/request", @"/v1/events", @"/v1/disconnect"] containsObject:path];
}
static inline BOOL SFNetworkPathPoll(NSString *path) {
  return [path isEqualToString:@"/v1/events"];
}
static inline BOOL SFNetworkPathFiniteCompletion(NSString *path) {
  return SFNetworkPathPoll(path) || [@[@"/v1/pair", @"/v1/disconnect"] containsObject:path ?: @""];
}
