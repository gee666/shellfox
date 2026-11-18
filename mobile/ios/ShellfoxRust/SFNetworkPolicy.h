#pragma once
#import <Foundation/Foundation.h>

// Exact carrier paths, shared by the legacy bridge and native Rust transport.
static inline BOOL SFNetworkPathAllowed(NSString *path) {
  if (![path isKindOfClass:NSString.class]) return NO;
  return [@[@"/v1/discover", @"/v1/pair", @"/v1/request", @"/v1/events", @"/v1/disconnect",
            @"/v2/discover", @"/v2/pair", @"/v2/request", @"/v2/events", @"/v2/disconnect"] containsObject:path];
}
static inline BOOL SFNetworkPathPoll(NSString *path) {
  return [path isEqualToString:@"/v1/events"] || [path isEqualToString:@"/v2/events"];
}
static inline BOOL SFNetworkPathFiniteCompletion(NSString *path) {
  return SFNetworkPathPoll(path) || [@[@"/v1/pair", @"/v2/pair", @"/v1/disconnect", @"/v2/disconnect"] containsObject:path ?: @""];
}
