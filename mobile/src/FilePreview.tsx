import React, { useMemo, useRef } from 'react';
import { View } from 'react-native';
import { WebView } from 'react-native-webview';
import { useUi } from './ui';
import { allowFilePreviewNavigation, filePreviewDocument } from './file-preview-document';

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
  return (
    <View style={styles.grow}>
      <WebView
        ref={web}
        testID="file-preview"
        accessibilityLabel={kind === 'html' ? 'HTML preview' : kind === 'svg' ? 'SVG preview' : kind === 'code' ? 'Code preview' : kind === 'image' ? 'Image preview' : 'Markdown preview'}
        style={[styles.grow, { backgroundColor: colors.bg }]}
        source={source}
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
        onError={event => onError(event.nativeEvent.description)}
        onContentProcessDidTerminate={() => onError('Preview stopped. Switch to source and reopen the preview.')}
        onRenderProcessGone={() => onError('Preview stopped. Switch to source and reopen the preview.')}
      />
    </View>
  );
}
