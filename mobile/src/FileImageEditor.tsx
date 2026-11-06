import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { Text, View } from 'react-native';
import { WebView, type WebViewMessageEvent } from 'react-native-webview';
import { Note, useUi } from './ui';
import { FileIconButton } from './FileIconButton';
import { allowImageEditorNavigation, type FileImageEditorProps, type ImageEditorCommand } from '../../src/shared/image-editor-protocol';
import { useImageEditor } from '../../src/shared/use-image-editor';

export type { FileImageEditorProps, ImageEditorFileAction } from '../../src/shared/image-editor-protocol';
export { discardImageEditorDraft, getImageDraftScope } from '../../src/shared/image-editor-drafts';

export function FileImageEditor(props: FileImageEditorProps) {
  const [recoveryKey, setRecoveryKey] = useState(0);
  const restart = useCallback(() => setRecoveryKey(value => value + 1), []);
  return <ImageEditorSession key={recoveryKey} {...props} restart={restart} />;
}

function ImageEditorSession({ restart, ...props }: FileImageEditorProps & { restart: () => void }) {
  const { colors, styles } = useUi();
  const [fatalMessage, setFatalMessage] = useState<string | null>(null);
  const fatal = useRef(false);
  const lastDocument = useRef<string | null>(null);
  const web = useRef<WebView>(null);
  const send = useCallback((command: ImageEditorCommand) => web.current?.postMessage(JSON.stringify(command)), []);
  const editor = useImageEditor(props, 'mobile', send);
  const receive = editor.receive;
  useEffect(() => {
    const previous = lastDocument.current;
    if (!fatal.current || !previous) lastDocument.current = editor.document;
    else if (editor.document && editor.document !== previous && !editor.busy) restart();
  }, [editor.document, editor.busy, restart]);
  const stopped = useCallback(() => {
    if (fatal.current) return;
    fatal.current = true;
    const message = 'Image editor stopped. Reopen it to restore your edits.';
    setFatalMessage(message);
    editor.transportFailed(message);
  }, [editor.transportFailed]);
  const onMessage = useCallback((event: WebViewMessageEvent) => {
    // Android reports this app-owned inline document as the opaque URL "null".
    // Authenticate its current token/schema; nativeEvent.url is not provenance.
    if (!fatal.current) receive(event.nativeEvent.data);
  }, [receive]);
  const source = useMemo(() => ({ html: editor.document ?? '', baseUrl: 'about:blank' }), [editor.document]);
  const showWebView = !!editor.document && !fatalMessage;
  const message = editor.error ?? fatalMessage;
  return <View style={[styles.grow, { position: 'relative' }]}>
    {!showWebView && (props.onFileAction || fatalMessage) && <View testID="image-editor-fallback-toolbar"
      style={{ height: 36, flexShrink: 0, flexDirection: 'row', alignItems: 'center', paddingHorizontal: 4, gap: 2,
        borderBottomWidth: 1, borderColor: colors.line, backgroundColor: colors.bg }}>
      {props.onFileAction && <FileIconButton icon="close" label="Close file" disabled={editor.busy} onPress={() => props.onFileAction?.('close')} />}
      <Text numberOfLines={1} style={{ flex: 1, minWidth: 0, color: colors.text, fontSize: 13 }}>{props.image.path.split(/[\\/]/).at(-1) || 'Image'}</Text>
      {fatalMessage && <FileIconButton icon="reload" label="Reopen image editor" disabled={editor.busy} busy={editor.busy} onPress={restart} />}
      {props.onFileAction && <FileIconButton icon="more" label="More file actions" disabled={editor.busy} onPress={() => props.onFileAction?.('more')} />}
    </View>}
    {!showWebView && !message && <Note>Loading image…</Note>}
    {message && (showWebView ? <View testID="image-editor-error-overlay" pointerEvents="none"
      style={{ position: 'absolute', bottom: 6, left: 6, right: 6, zIndex: 2, maxHeight: '50%', overflow: 'hidden',
        padding: 6, borderRadius: 3, backgroundColor: colors.panel }}><Note error>{message}</Note></View> : <Note error>{message}</Note>)}
    {showWebView && <WebView ref={web} testID="image-editor" accessibilityLabel="Image editing canvas"
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
      onContentProcessDidTerminate={stopped}
      onRenderProcessGone={stopped}
    />}
  </View>;
}
