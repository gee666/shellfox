#import <Foundation/Foundation.h>
#import <UIKit/UIKit.h>
#import <PhotosUI/PhotosUI.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>
#import <React/RCTBridgeModule.h>
#import <React/RCTInvalidating.h>
#import <React/RCTUtils.h>

static const NSUInteger SFMaxFile = 64 * 1024 * 1024;
static const NSUInteger SFMaxChunk = 192 * 1024;

@interface ShellfoxFiles : NSObject <RCTBridgeModule, RCTInvalidating, UIDocumentPickerDelegate, PHPickerViewControllerDelegate, UIAdaptivePresentationControllerDelegate>
@property(nonatomic, strong) dispatch_queue_t worker;
@property(nonatomic, strong) NSMutableDictionary<NSString *, NSURL *> *handles;
@property(nonatomic, strong) NSURL *cache;
@property(nonatomic, copy) NSString *token;
@property(nonatomic, copy) RCTPromiseResolveBlock pickResolve;
@property(nonatomic, copy) RCTPromiseRejectBlock pickReject;
@property(nonatomic, strong) UIViewController *picker;
@property(nonatomic, strong) NSFileCoordinator *coordinator;
@property(nonatomic, strong) NSProgress *photoProgress;
@property(nonatomic) BOOL closed;
@end

@implementation ShellfoxFiles
RCT_EXPORT_MODULE(ShellfoxFiles)
+ (BOOL)requiresMainQueueSetup { return NO; }
- (instancetype)init {
  if ((self = [super init])) {
    _worker = dispatch_queue_create("com.shellfox.files", DISPATCH_QUEUE_SERIAL);
    _handles = [NSMutableDictionary new];
    _cache = [[NSURL fileURLWithPath:NSTemporaryDirectory() isDirectory:YES] URLByAppendingPathComponent:@"terminal-attachments" isDirectory:YES];
    dispatch_async(_worker, ^{
      [[NSFileManager defaultManager] removeItemAtURL:self.cache error:nil];
      [[NSFileManager defaultManager] createDirectoryAtURL:self.cache withIntermediateDirectories:YES attributes:nil error:nil];
    });
  }
  return self;
}
- (BOOL)validID:(NSString *)value { return [[NSUUID alloc] initWithUUIDString:value] != nil; }
- (BOOL)active:(NSString *)token { @synchronized(self) { return !self.closed && [self.token isEqualToString:token]; } }
- (void)complete:(NSString *)token value:(id)value error:(NSError *)error {
  @synchronized(self) {
    if (![self.token isEqualToString:token]) return;
    RCTPromiseResolveBlock resolve = self.pickResolve;
    RCTPromiseRejectBlock reject = self.pickReject;
    self.token = nil; self.pickResolve = nil; self.pickReject = nil;
    [self.photoProgress cancel]; self.photoProgress = nil;
    [self.coordinator cancel];
    if (error) reject(@"FILE_PICK", error.localizedDescription, error); else resolve(value);
  }
}
- (NSError *)fileError:(NSString *)message { return [NSError errorWithDomain:@"ShellfoxFiles" code:1 userInfo:@{NSLocalizedDescriptionKey:message}]; }
- (void)present:(UIViewController *)picker {
  self.picker = picker;
  picker.presentationController.delegate = self;
  [RCTPresentedViewController() presentViewController:picker animated:YES completion:^{ picker.presentationController.delegate = self; }];
}
- (void)replaceChoiceWith:(UIViewController *)picker token:(NSString *)token {
  // Present only after the action sheet has left the view hierarchy.
  [self.picker dismissViewControllerAnimated:YES completion:^{
    if ([self active:token]) [self present:picker];
  }];
}
RCT_REMAP_METHOD(pick, pickToken:(NSString *)token resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:token]) { reject(@"VALIDATION", @"Invalid picker token.", nil); return; }
  dispatch_async(dispatch_get_main_queue(), ^{
    @synchronized(self) {
      if (self.closed || self.token || self.handles.count >= 4 || !RCTPresentedViewController()) {
        reject(@"PICKER_UNAVAILABLE", @"The file picker is unavailable or already open.", nil); return;
      }
      self.token = token; self.pickResolve = resolve; self.pickReject = reject;
    }
    UIAlertController *choice = [UIAlertController alertControllerWithTitle:@"Attach" message:nil preferredStyle:UIAlertControllerStyleActionSheet];
    [choice addAction:[UIAlertAction actionWithTitle:@"Files" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) {
      if (![self active:token]) return;
      UIDocumentPickerViewController *picker = [[UIDocumentPickerViewController alloc] initForOpeningContentTypes:@[UTTypeItem] asCopy:NO];
      picker.allowsMultipleSelection = NO; picker.delegate = self;
      [self replaceChoiceWith:picker token:token];
    }]];
    [choice addAction:[UIAlertAction actionWithTitle:@"Photos" style:UIAlertActionStyleDefault handler:^(UIAlertAction *action) {
      if (![self active:token]) return;
      PHPickerConfiguration *config = [[PHPickerConfiguration alloc] init];
      config.selectionLimit = 1; config.filter = [PHPickerFilter imagesFilter];
      PHPickerViewController *picker = [[PHPickerViewController alloc] initWithConfiguration:config];
      picker.delegate = self; [self replaceChoiceWith:picker token:token];
    }]];
    [choice addAction:[UIAlertAction actionWithTitle:@"Cancel" style:UIAlertActionStyleCancel handler:^(UIAlertAction *action) { self.picker = nil; [self complete:token value:[NSNull null] error:nil]; }]];
    UIView *view = RCTPresentedViewController().view;
    choice.popoverPresentationController.sourceView = view;
    choice.popoverPresentationController.sourceRect = CGRectMake(CGRectGetMidX(view.bounds), CGRectGetMaxY(view.bounds), 1, 1);
    [self present:choice];
  });
}

// Coordinate cloud materialization and measure actual bytes instead of trusting provider metadata.
- (void)copyURL:(NSURL *)url token:(NSString *)token {
  if (![self active:token]) return;
  BOOL scoped = [url startAccessingSecurityScopedResource];
  NSURL *destination = [self.cache URLByAppendingPathComponent:[NSUUID UUID].UUIDString];
  __block NSError *failure = nil;
  __block NSUInteger size = 0;
  NSFileCoordinator *coordinator = [[NSFileCoordinator alloc] initWithFilePresenter:nil];
  @try {
    @synchronized(self) {
      if (![self active:token]) return;
      self.coordinator = coordinator;
    }
    [coordinator coordinateReadingItemAtURL:url options:NSFileCoordinatorReadingWithoutChanges error:&failure byAccessor:^(NSURL *readURL) {
      NSNumber *regular = nil;
      if (![readURL getResourceValue:&regular forKey:NSURLIsRegularFileKey error:&failure] || !regular.boolValue) {
        if (!failure) failure = [self fileError:@"Choose a file, not a folder."]; return;
      }
      NSInputStream *input = [NSInputStream inputStreamWithURL:readURL];
      NSOutputStream *output = [NSOutputStream outputStreamWithURL:destination append:NO];
      [input open]; [output open];
      @try {
        uint8_t buffer[65536];
        while ([self active:token]) {
          NSInteger count = [input read:buffer maxLength:sizeof(buffer)];
          if (count < 0) { failure = input.streamError ?: [self fileError:@"Could not read the selected file."]; break; }
          if (count == 0) break;
          size += count;
          if (size > SFMaxFile) { failure = [self fileError:@"Choose a file no larger than 64 MiB."]; break; }
          NSInteger written = 0;
          while (written < count) {
            NSInteger n = [output write:buffer + written maxLength:count - written];
            if (n <= 0) { failure = output.streamError ?: [self fileError:@"Could not cache the selected file."]; break; }
            written += n;
          }
          if (failure) break;
        }
      } @finally { [input close]; [output close]; }
    }];
    if (![self active:token] || failure) {
      [[NSFileManager defaultManager] removeItemAtURL:destination error:nil];
      [self complete:token value:nil error:failure]; return;
    }
    NSString *name = url.lastPathComponent.length ? url.lastPathComponent : @"attachment";
    name = [[name componentsSeparatedByCharactersInSet:[NSCharacterSet controlCharacterSet]] componentsJoinedByString:@"_"];
    if (name.length > 1024) name = [name substringToIndex:1024];
    @synchronized(self) {
      if (![self active:token]) { [[NSFileManager defaultManager] removeItemAtURL:destination error:nil]; return; }
      NSString *handle = [NSUUID UUID].UUIDString;
      self.handles[handle] = destination;
      [self complete:token value:@{@"handle":handle, @"fileName":name, @"size":@(size)} error:nil];
    }
  } @finally {
    @synchronized(self) { if (self.coordinator == coordinator) self.coordinator = nil; }
    if (scoped) [url stopAccessingSecurityScopedResource];
  }
}
- (void)documentPicker:(UIDocumentPickerViewController *)controller didPickDocumentsAtURLs:(NSArray<NSURL *> *)urls {
  if (controller != self.picker) return;
  NSString *token = self.token;
  self.picker = nil;
  if (urls.count != 1) { [self complete:token value:[NSNull null] error:nil]; return; }
  dispatch_async(self.worker, ^{ [self copyURL:urls.firstObject token:token]; });
}
- (void)documentPickerWasCancelled:(UIDocumentPickerViewController *)controller { if (controller != self.picker) return; self.picker = nil; [self complete:self.token value:[NSNull null] error:nil]; }
- (void)presentationControllerDidDismiss:(UIPresentationController *)presentationController { if (presentationController.presentedViewController != self.picker) return; self.picker = nil; [self complete:self.token value:[NSNull null] error:nil]; }
- (void)picker:(PHPickerViewController *)picker didFinishPicking:(NSArray<PHPickerResult *> *)results {
  if (picker != self.picker) return;
  NSString *token = self.token;
  [picker dismissViewControllerAnimated:YES completion:nil]; self.picker = nil;
  if (results.count != 1) { [self complete:token value:[NSNull null] error:nil]; return; }
  NSProgress *loading = [results.firstObject.itemProvider loadFileRepresentationForTypeIdentifier:UTTypeImage.identifier completionHandler:^(NSURL *url, NSError *error) {
    if (error || !url) { [self complete:token value:nil error:error ?: [self fileError:@"Could not download the selected photo."]]; return; }
    // The provider deletes this URL when the completion handler returns.
    dispatch_sync(self.worker, ^{ [self copyURL:url token:token]; });
  }];
  @synchronized(self) { if ([self active:token]) self.photoProgress = loading; else [loading cancel]; }
}
RCT_REMAP_METHOD(cancelPick, cancelToken:(NSString *)token resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:token]) { reject(@"VALIDATION", @"Invalid picker token.", nil); return; }
  dispatch_async(dispatch_get_main_queue(), ^{
    if ([self active:token]) {
      [self complete:token value:[NSNull null] error:nil];
      [self.picker dismissViewControllerAnimated:YES completion:nil]; self.picker = nil;
    }
    resolve([NSNull null]);
  });
}
RCT_REMAP_METHOD(read, readHandle:(NSString *)handle offset:(double)offset length:(double)length resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:handle] || !isfinite(offset) || offset < 0 || offset > SFMaxFile || floor(offset) != offset || !isfinite(length) || floor(length) != length || length < 1 || length > SFMaxChunk) {
    reject(@"VALIDATION", @"Invalid file chunk.", nil); return;
  }
  dispatch_async(self.worker, ^{
    NSURL *url;
    @synchronized(self) { url = self.closed ? nil : self.handles[handle]; }
    if (!url) { reject(@"FILE_READ", @"The selected file was released.", nil); return; }
    NSError *error = nil;
    NSFileHandle *input = [NSFileHandle fileHandleForReadingFromURL:url error:&error];
    if (!input) { reject(@"FILE_READ", error.localizedDescription, error); return; }
    NSData *data = nil;
    @try {
      if ([input seekToOffset:(unsigned long long)offset error:&error]) data = [input readDataUpToLength:(NSUInteger)length error:&error];
    } @finally { [input closeAndReturnError:nil]; }
    if (!data || data.length != (NSUInteger)length || error) { reject(@"FILE_READ", @"The selected file changed or could not be read completely.", error); return; }
    resolve(@{@"data":[data base64EncodedStringWithOptions:0], @"bytes":@(data.length)});
  });
}
RCT_REMAP_METHOD(release, releaseHandle:(NSString *)handle resolver:(RCTPromiseResolveBlock)resolve rejecter:(RCTPromiseRejectBlock)reject) {
  if (![self validID:handle]) { reject(@"VALIDATION", @"Invalid file handle.", nil); return; }
  dispatch_async(self.worker, ^{
    NSURL *url;
    @synchronized(self) { url = self.handles[handle]; [self.handles removeObjectForKey:handle]; }
    NSError *error = nil;
    if (url && ![[NSFileManager defaultManager] removeItemAtURL:url error:&error] && error.code != NSFileNoSuchFileError) reject(@"FILE_RELEASE", error.localizedDescription, error);
    else resolve([NSNull null]);
  });
}
- (void)invalidate {
  @synchronized(self) { self.closed = YES; [self complete:self.token value:[NSNull null] error:nil]; }
  dispatch_async(dispatch_get_main_queue(), ^{ [self.picker dismissViewControllerAnimated:NO completion:nil]; self.picker = nil; });
  dispatch_async(self.worker, ^{
    @synchronized(self) { [self.handles removeAllObjects]; }
    [[NSFileManager defaultManager] removeItemAtURL:self.cache error:nil];
  });
}
@end
