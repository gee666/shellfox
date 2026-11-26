import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { View } from 'react-native';
import { WebView } from 'react-native-webview';
import { Button, Note, useUi } from './ui';
import { allowFilePreviewNavigation, filePreviewDocument } from './file-preview-document';

// Local HTML paints in well under a second. A WebKit content process that never
// starts or dies leaves the view blank without any error, so a load that takes
// this long is retried in a new WebView.
export const PREVIEW_LOAD_TIMEOUT_MS = 10_000;
const PREVIEW_ATTEMPTS = 3;

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
  const report = useRef(onError);
  report.current = onError;
  // The page lives in a WebKit process whose accessibility can come up long
  // after the document. Report the load natively so a reader knows it is ready.
  const [state, setState] = useState<'loading' | 'loaded' | 'failed'>('loading');
  // Bumped to replace the native view. Attempts are bounded per document, so a
  // page that kills its content process cannot reload forever.
  const [generation, setGeneration] = useState(0);
  const attempts = useRef(1);
  const restart = useCallback((message: string) => {
    if (attempts.current >= PREVIEW_ATTEMPTS) { setState('failed'); report.current(message); return; }
    attempts.current += 1;
    setState('loading'); setGeneration(value => value + 1);
  }, []);
  useEffect(() => { attempts.current = 1; setState('loading'); }, [source]);
  useEffect(() => {
    if (state !== 'loading') return;
    const timer = setTimeout(() => restart('The preview did not load.'), PREVIEW_LOAD_TIMEOUT_MS);
    return () => clearTimeout(timer);
  }, [state, generation, source, restart]);
  const stopped = () => restart('Preview stopped. Switch to source and reopen the preview.');
  if (state === 'failed') {
    return <View style={[styles.grow, { alignItems: 'center', justifyContent: 'center', gap: 8 }]}>
      <Note error>Preview did not load.</Note>
      <Button title="Retry" onPress={() => { attempts.current = 1; setState('loading'); setGeneration(value => value + 1); }} />
    </View>;
  }
  return (
    <View style={styles.grow}>
      <WebView
        key={generation}
        ref={web}
        testID="file-preview"
        accessibilityLabel={kind === 'html' ? 'HTML preview' : kind === 'svg' ? 'SVG preview' : kind === 'code' ? 'Code preview' : kind === 'image' ? 'Image preview' : 'Markdown preview'}
        accessibilityValue={{ text: state === 'loaded' ? 'Loaded' : 'Loading' }}
        style={[styles.grow, { backgroundColor: colors.bg }]}
        source={source}
        // onLoad, not onLoadEnd: onLoadEnd also follows a failed load.
        onLoad={() => setState('loaded')}
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
          if (state === 'loaded') onError(event.nativeEvent.description);
          else restart(event.nativeEvent.description);
        }}
        onContentProcessDidTerminate={stopped}
        onRenderProcessGone={stopped}
      />
    </View>
  );
}
