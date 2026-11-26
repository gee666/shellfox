import React, { useMemo, useRef } from 'react';
import { View } from 'react-native';
import { WebView } from 'react-native-webview';
import { Button, Note, useUi } from './ui';
import { allowFilePreviewNavigation, filePreviewDocument } from './file-preview-document';
import { useRecoveringWebView } from './useRecoveringWebView';

export function FilePreview({ text, kind, path, preparedMarkdown, wrapLines = true, onError }: {
  text: string;
  kind: 'markdown' | 'html' | 'svg' | 'code' | 'image';
  path?: string;
  preparedMarkdown?: string;
  wrapLines?: boolean;
  onError: (message: string) => void;
}) {
  const { colors, styles } = useUi();
  const web = useRef<WebView>(null);
  const source = useMemo(() => ({
    html: filePreviewDocument(text, kind, colors.bg, colors.text, preparedMarkdown, path, wrapLines), baseUrl: 'about:blank',
  }), [text, kind, colors.bg, colors.text, preparedMarkdown, path, wrapLines]);
  // The page lives in a WebKit process whose accessibility can come up long
  // after the document. Report the load natively so a reader knows it is ready.
  const view = useRecoveringWebView(source, { crash: 'replace', onGiveUp: failure => onError(
    failure.cause === 'timeout' ? 'The preview did not load.'
      : failure.cause === 'stopped' ? 'Preview stopped. Switch to source and reopen the preview.' : failure.message) });
  if (view.state === 'failed') {
    return <View style={[styles.grow, { alignItems: 'center', justifyContent: 'center', gap: 8 }]}>
      <Note error>Preview did not load.</Note>
      <Button title="Retry" onPress={view.retry} />
    </View>;
  }
  return (
    <View style={styles.grow}>
      <WebView
        key={view.key}
        ref={web}
        testID="file-preview"
        accessibilityLabel={kind === 'html' ? 'HTML preview' : kind === 'svg' ? 'SVG preview' : kind === 'code' ? 'Code preview' : kind === 'image' ? 'Image preview' : 'Markdown preview'}
        accessibilityValue={{ text: view.state === 'loaded' ? 'Loaded' : 'Loading' }}
        style={[styles.grow, { backgroundColor: colors.bg }]}
        source={source}
        {...view.handlers}
        // Do not attach onMessage. File scripts must never receive a native bridge.
        originWhitelist={['*']}
        onShouldStartLoadWithRequest={request => allowFilePreviewNavigation(request.url)}
        onNavigationStateChange={state => {
          if (!allowFilePreviewNavigation(state.url)) web.current?.stopLoading();
        }}
        onOpenWindow={() => undefined}
        onFileDownload={() => undefined}
        javaScriptEnabled={kind === 'html'}
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
        onError={event => {
          // A failed first load leaves the view blank. Later errors keep the page.
          if (view.state === 'loaded') onError(event.nativeEvent.description);
          else view.replace({ cause: 'error', message: event.nativeEvent.description });
        }}
      />
    </View>
  );
}
