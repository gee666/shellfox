import React, { useCallback, useMemo, useRef } from 'react';
import { View } from 'react-native';
import { WebView, type WebViewMessageEvent } from 'react-native-webview';
import { Note, useUi } from './ui';
import { allowImageEditorNavigation, type FileImageEditorProps, type ImageEditorCommand } from '../../src/shared/image-editor-protocol';
import { useImageEditor } from '../../src/shared/use-image-editor';

export type { FileImageEditorProps } from '../../src/shared/image-editor-protocol';
export { discardImageEditorDraft, getImageDraftScope } from '../../src/shared/image-editor-drafts';

export function FileImageEditor(props: FileImageEditorProps) {
  const { colors, styles } = useUi();
  const web = useRef<WebView>(null);
  const send = useCallback((command: ImageEditorCommand) => web.current?.postMessage(JSON.stringify(command)), []);
  const editor = useImageEditor(props, 'mobile', send);
  const receive = editor.receive;
  const onMessage = useCallback((event: WebViewMessageEvent) => {
    // Android reports this app-owned inline document as the opaque URL "null".
    // Authenticate its current token/schema; nativeEvent.url is not provenance.
    receive(event.nativeEvent.data);
  }, [receive]);
  const source = useMemo(() => ({ html: editor.document ?? '', baseUrl: 'about:blank' }), [editor.document]);
  return <View style={styles.grow}>
    {editor.error && <Note error>{editor.error}</Note>}
    {editor.document && <WebView ref={web} testID="image-editor" accessibilityLabel="Image editing canvas"
      style={[styles.grow, { backgroundColor: colors.bg }]} source={source} onMessage={onMessage}
      originWhitelist={['*']}
      onShouldStartLoadWithRequest={request => allowImageEditorNavigation(request.url)}
      onNavigationStateChange={state => {
        if (!allowImageEditorNavigation(state.url)) web.current?.stopLoading();
      }}
      onOpenWindow={() => undefined}
      onFileDownload={() => undefined}
      javaScriptEnabled
      javaScriptCanOpenWindowsAutomatically={false}
      setSupportMultipleWindows={false}
      allowFileAccess={false}
      allowFileAccessFromFileURLs={false}
      allowUniversalAccessFromFileURLs={false}
      allowingReadAccessToURL="about:blank"
      mixedContentMode="never"
      thirdPartyCookiesEnabled={false}
      sharedCookiesEnabled={false}
      domStorageEnabled={false}
      cacheEnabled={false}
      incognito
      allowsInlineMediaPlayback={false}
      mediaPlaybackRequiresUserAction
      allowsLinkPreview={false}
      dataDetectorTypes={['none']}
      automaticallyAdjustContentInsets={false}
      contentInsetAdjustmentBehavior="never"
      onError={event => editor.fail(event.nativeEvent.description)}
      onContentProcessDidTerminate={() => editor.fail('Image editor stopped. Reopen the image to restore your edits.')}
      onRenderProcessGone={() => editor.fail('Image editor stopped. Reopen the image to restore your edits.')}
    />}
  </View>;
}
