/** The webview's OS. Windows (WebView2) needs no Accessibility grant, so its UI hides that control. */
export const IS_WINDOWS = typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);
