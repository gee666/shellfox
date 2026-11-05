#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#import <React/RCTBridgeModule.h>
#import <React/RCTInvalidating.h>
#import <React/RCTUtils.h>

static const NSUInteger SFDownloadLimit = 1024ULL * 1024 * 1024;
static const NSUInteger SFDownloadChunk = 192 * 1024;
@interface SFDownload : NSObject
@property(nonatomic, strong) NSURL *url;
@property(nonatomic, strong) NSFileHandle *file;
@property(nonatomic, copy) NSString *name;
@property(nonatomic) NSUInteger size;
@property(nonatomic) NSUInteger offset;
@property(nonatomic) NSTimeInterval touched;
@end
@implementation SFDownload
@end

/** iOS has no shared Android-style Downloads directory. Export through the Files picker. */
@interface ShellfoxDownloads : NSObject <RCTBridgeModule, RCTInvalidating, UIDocumentPickerDelegate, UIAdaptivePresentationControllerDelegate>
@property(nonatomic, strong) dispatch_queue_t worker;
@property(nonatomic, strong) dispatch_source_t expiry;
@property(nonatomic, strong) NSMutableDictionary<NSString *, SFDownload *> *downloads;
@property(nonatomic, strong) NSURL *cache;
@property(nonatomic, strong) UIDocumentPickerViewController *picker;
@property(nonatomic, copy) NSString *exportHandle;
@property(nonatomic, copy) RCTPromiseResolveBlock exportResolve;
@property(atomic) BOOL closed;
@end

@implementation ShellfoxDownloads
RCT_EXPORT_MODULE(ShellfoxDownloads)
+ (BOOL)requiresMainQueueSetup { return NO; }
- (instancetype)init {
  if ((self = [super init])) {
    _worker = dispatch_queue_create("com.shellfox.downloads", DISPATCH_QUEUE_SERIAL);
    _downloads = [NSMutableDictionary new];
    _cache = [[NSURL fileURLWithPath:NSTemporaryDirectory() isDirectory:YES] URLByAppendingPathComponent:@"shellfox-downloads" isDirectory:YES];
    dispatch_async(_worker, ^{
      // Staged files are never resumable across an app restart.
      [[NSFileManager defaultManager] removeItemAtURL:self.cache error:nil];
      [[NSFileManager defaultManager] createDirectoryAtURL:self.cache withIntermediateDirectories:YES attributes:@{NSFileProtectionKey:NSFileProtectionCompleteUntilFirstUserAuthentication} error:nil];
    });
    _expiry = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, _worker);
    dispatch_source_set_timer(_expiry, dispatch_time(DISPATCH_TIME_NOW, 60 * NSEC_PER_SEC), 60 * NSEC_PER_SEC, NSEC_PER_SEC);
    __weak ShellfoxDownloads *weakSelf = self;
    dispatch_source_set_event_handler(_expiry, ^{
      ShellfoxDownloads *owner = weakSelf;
      if (!owner) return;
      for (NSString *handle in owner.downloads.allKeys) {
        SFDownload *current = owner.downloads[handle];
        NSString *exporting;
        @synchronized(owner) { exporting = owner.exportHandle; }
        if (![handle isEqualToString:exporting] && NSDate.timeIntervalSinceReferenceDate - current.touched > 300) [owner remove:handle];
      }
    });
    dispatch_resume(_expiry);
  }
  return self;
}
- (BOOL)validID:(NSString *)value { return [[NSUUID alloc] initWithUUIDString:value] != nil; }
- (void)remove:(NSString *)handle {
  SFDownload *current = self.downloads[handle];
  [self.downloads removeObjectForKey:handle];
  [current.file closeAndReturnError:nil];
  if (current.url) [[NSFileManager defaultManager] removeItemAtURL:current.url.URLByDeletingLastPathComponent error:nil];
}
RCT_REMAP_METHOD(begin, beginToken:(NSString *)token fileName:(NSString *)fileName size:(double)size mimeType:(NSString *)mimeType resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  NSCharacterSet *unsafe = [NSCharacterSet characterSetWithCharactersInString:@"/\\"];
  if (![self validID:token] || !fileName.length || fileName.length > 255 || [fileName isEqualToString:@"."] || [fileName isEqualToString:@".."] ||
      [fileName rangeOfCharacterFromSet:unsafe].location != NSNotFound || [fileName rangeOfCharacterFromSet:NSCharacterSet.controlCharacterSet].location != NSNotFound ||
      !isfinite(size) || size < 0 || size > SFDownloadLimit || floor(size) != size) {
    reject(@"VALIDATION", @"Invalid download name or size.", nil); return;
  }
  dispatch_async(self.worker, ^{
    if (self.closed || self.downloads.count >= 4 || self.downloads[token]) { reject(@"DOWNLOAD_CLOSED", @"Downloads are unavailable or already open.", nil); return; }
    NSURL *folder = [self.cache URLByAppendingPathComponent:token isDirectory:YES];
    NSURL *url = [folder URLByAppendingPathComponent:fileName];
    NSError *error = nil;
    [[NSFileManager defaultManager] createDirectoryAtURL:folder withIntermediateDirectories:YES attributes:@{NSFileProtectionKey:NSFileProtectionCompleteUntilFirstUserAuthentication} error:&error];
    if (!error && ![[NSFileManager defaultManager] createFileAtPath:url.path contents:nil attributes:nil]) error = [NSError errorWithDomain:@"ShellfoxDownloads" code:1 userInfo:@{NSLocalizedDescriptionKey:@"Could not stage the download."}];
    NSFileHandle *file = error ? nil : [NSFileHandle fileHandleForWritingToURL:url error:&error];
    if (!file) { [[NSFileManager defaultManager] removeItemAtURL:folder error:nil]; reject(@"DOWNLOAD_IO", error.localizedDescription, error); return; }
    SFDownload *current = [SFDownload new];
    current.url = url; current.file = file; current.name = fileName; current.size = (NSUInteger)size; current.touched = NSDate.timeIntervalSinceReferenceDate;
    self.downloads[token] = current;
    resolve(@{@"handle":token});
  });
}
RCT_REMAP_METHOD(write, writeHandle:(NSString *)handle offset:(double)offset data:(NSString *)encoded resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:handle] || !isfinite(offset) || offset < 0 || offset > SFDownloadLimit || floor(offset) != offset || encoded.length > SFDownloadChunk * 4 / 3) {
    reject(@"VALIDATION", @"Invalid download chunk.", nil); return;
  }
  dispatch_async(self.worker, ^{
    SFDownload *current = self.closed ? nil : self.downloads[handle];
    NSData *data = [[NSData alloc] initWithBase64EncodedString:encoded options:0];
    if (!current || current.offset != (NSUInteger)offset || !data.length || data.length > SFDownloadChunk || current.offset + data.length > current.size || ![[data base64EncodedStringWithOptions:0] isEqualToString:encoded]) {
      reject(@"VALIDATION", @"Unexpected download offset or data.", nil); return;
    }
    NSError *error = nil;
    if (![current.file writeData:data error:&error]) { reject(@"DOWNLOAD_IO", error.localizedDescription, error); return; }
    current.offset += data.length; current.touched = NSDate.timeIntervalSinceReferenceDate;
    resolve(@{@"offset":@(current.offset)});
  });
}
- (void)completeExport:(BOOL)saved {
  NSString *handle; RCTPromiseResolveBlock resolve;
  @synchronized(self) {
    handle = self.exportHandle; resolve = self.exportResolve;
    self.exportHandle = nil; self.exportResolve = nil; self.picker = nil;
  }
  if (!handle) return;
  dispatch_async(self.worker, ^{
    SFDownload *current = self.downloads[handle];
    if (resolve) resolve(saved ? @{@"fileName":current.name ?: @"download", @"location":@"Files"} : [NSNull null]);
    [self remove:handle];
  });
}
RCT_REMAP_METHOD(finish, finishHandle:(NSString *)handle resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:handle]) { reject(@"VALIDATION", @"Invalid download handle.", nil); return; }
  dispatch_async(self.worker, ^{
    SFDownload *current = self.closed ? nil : self.downloads[handle];
    if (!current || current.offset != current.size) { reject(@"DOWNLOAD_IO", @"The download is incomplete or cancelled.", nil); return; }
    NSError *error = nil;
    if (![current.file synchronizeAndReturnError:&error] || ![current.file closeAndReturnError:&error]) { reject(@"DOWNLOAD_IO", error.localizedDescription, error); return; }
    current.file = nil;
    dispatch_async(dispatch_get_main_queue(), ^{
      @synchronized(self) {
        if (self.closed || self.exportHandle || !RCTPresentedViewController()) { reject(@"DOWNLOAD_PICKER", @"The Files export picker is unavailable.", nil); return; }
        self.exportHandle = handle; self.exportResolve = resolve;
        self.picker = [[UIDocumentPickerViewController alloc] initForExportingURLs:@[current.url] asCopy:YES];
        self.picker.delegate = self;
        [RCTPresentedViewController() presentViewController:self.picker animated:YES completion:^{ self.picker.presentationController.delegate = self; }];
      }
    });
  });
}
RCT_REMAP_METHOD(cancel, cancelHandle:(NSString *)handle resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:handle]) { reject(@"VALIDATION", @"Invalid download handle.", nil); return; }
  dispatch_async(dispatch_get_main_queue(), ^{
    @synchronized(self) {
      if ([self.exportHandle isEqualToString:handle]) { [self.picker dismissViewControllerAnimated:YES completion:nil]; [self completeExport:NO]; }
    }
    dispatch_async(self.worker, ^{ [self remove:handle]; resolve([NSNull null]); });
  });
}
- (void)documentPicker:(UIDocumentPickerViewController *)controller didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls { if (controller == self.picker) [self completeExport:urls.count == 1]; }
- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller { if (controller == self.picker) [self completeExport:NO]; }
- (void)presentationControllerDidDismiss:(UIPresentationController *)controller { if (controller.presentedViewController == self.picker) [self completeExport:NO]; }
- (void)invalidate {
  @synchronized(self) { self.closed = YES; }
  dispatch_source_cancel(self.expiry);
  dispatch_async(dispatch_get_main_queue(), ^{ [self.picker dismissViewControllerAnimated:NO completion:nil]; [self completeExport:NO]; });
  dispatch_async(self.worker, ^{ for (NSString *handle in self.downloads.allKeys) [self remove:handle]; });
}
@end
