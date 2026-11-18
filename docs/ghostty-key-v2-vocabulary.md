# Proposed KeyV2 physical vocabulary

Proposal only. These names do not introduce a new encoder or modify any current
manager/V1/Observer schema. Russell owns the corresponding closed manager-v2
vocabulary after review.

This table was mechanically extracted from signed public libghostty-vt 1.0.1
`key/event.h`, SHA-256
`4a615f9864ae11bafa06cce7493aa6537804750ed4a6fda8b3f962ea348a8bc9`.
All 176 numeric values are consecutive, 0 through 175. The MAX_VALUE enum sentinel
is not a physical key. Rust names use W3C-style `KeyA` through `KeyZ` to avoid
confusing writing-system KeyF with function key F25. Numeric conversion is checked;
there is no arbitrary character/string fallback to zero.

| Code | Proposed Rust/manager-v2 name | Exact public enumerator |
| --- | --- | --- |
| 0 | `Unidentified` | `GHOSTTY_KEY_UNIDENTIFIED` |
| 1 | `Backquote` | `GHOSTTY_KEY_BACKQUOTE` |
| 2 | `Backslash` | `GHOSTTY_KEY_BACKSLASH` |
| 3 | `BracketLeft` | `GHOSTTY_KEY_BRACKET_LEFT` |
| 4 | `BracketRight` | `GHOSTTY_KEY_BRACKET_RIGHT` |
| 5 | `Comma` | `GHOSTTY_KEY_COMMA` |
| 6 | `Digit0` | `GHOSTTY_KEY_DIGIT_0` |
| 7 | `Digit1` | `GHOSTTY_KEY_DIGIT_1` |
| 8 | `Digit2` | `GHOSTTY_KEY_DIGIT_2` |
| 9 | `Digit3` | `GHOSTTY_KEY_DIGIT_3` |
| 10 | `Digit4` | `GHOSTTY_KEY_DIGIT_4` |
| 11 | `Digit5` | `GHOSTTY_KEY_DIGIT_5` |
| 12 | `Digit6` | `GHOSTTY_KEY_DIGIT_6` |
| 13 | `Digit7` | `GHOSTTY_KEY_DIGIT_7` |
| 14 | `Digit8` | `GHOSTTY_KEY_DIGIT_8` |
| 15 | `Digit9` | `GHOSTTY_KEY_DIGIT_9` |
| 16 | `Equal` | `GHOSTTY_KEY_EQUAL` |
| 17 | `IntlBackslash` | `GHOSTTY_KEY_INTL_BACKSLASH` |
| 18 | `IntlRo` | `GHOSTTY_KEY_INTL_RO` |
| 19 | `IntlYen` | `GHOSTTY_KEY_INTL_YEN` |
| 20 | `KeyA` | `GHOSTTY_KEY_A` |
| 21 | `KeyB` | `GHOSTTY_KEY_B` |
| 22 | `KeyC` | `GHOSTTY_KEY_C` |
| 23 | `KeyD` | `GHOSTTY_KEY_D` |
| 24 | `KeyE` | `GHOSTTY_KEY_E` |
| 25 | `KeyF` | `GHOSTTY_KEY_F` |
| 26 | `KeyG` | `GHOSTTY_KEY_G` |
| 27 | `KeyH` | `GHOSTTY_KEY_H` |
| 28 | `KeyI` | `GHOSTTY_KEY_I` |
| 29 | `KeyJ` | `GHOSTTY_KEY_J` |
| 30 | `KeyK` | `GHOSTTY_KEY_K` |
| 31 | `KeyL` | `GHOSTTY_KEY_L` |
| 32 | `KeyM` | `GHOSTTY_KEY_M` |
| 33 | `KeyN` | `GHOSTTY_KEY_N` |
| 34 | `KeyO` | `GHOSTTY_KEY_O` |
| 35 | `KeyP` | `GHOSTTY_KEY_P` |
| 36 | `KeyQ` | `GHOSTTY_KEY_Q` |
| 37 | `KeyR` | `GHOSTTY_KEY_R` |
| 38 | `KeyS` | `GHOSTTY_KEY_S` |
| 39 | `KeyT` | `GHOSTTY_KEY_T` |
| 40 | `KeyU` | `GHOSTTY_KEY_U` |
| 41 | `KeyV` | `GHOSTTY_KEY_V` |
| 42 | `KeyW` | `GHOSTTY_KEY_W` |
| 43 | `KeyX` | `GHOSTTY_KEY_X` |
| 44 | `KeyY` | `GHOSTTY_KEY_Y` |
| 45 | `KeyZ` | `GHOSTTY_KEY_Z` |
| 46 | `Minus` | `GHOSTTY_KEY_MINUS` |
| 47 | `Period` | `GHOSTTY_KEY_PERIOD` |
| 48 | `Quote` | `GHOSTTY_KEY_QUOTE` |
| 49 | `Semicolon` | `GHOSTTY_KEY_SEMICOLON` |
| 50 | `Slash` | `GHOSTTY_KEY_SLASH` |
| 51 | `AltLeft` | `GHOSTTY_KEY_ALT_LEFT` |
| 52 | `AltRight` | `GHOSTTY_KEY_ALT_RIGHT` |
| 53 | `Backspace` | `GHOSTTY_KEY_BACKSPACE` |
| 54 | `CapsLock` | `GHOSTTY_KEY_CAPS_LOCK` |
| 55 | `ContextMenu` | `GHOSTTY_KEY_CONTEXT_MENU` |
| 56 | `ControlLeft` | `GHOSTTY_KEY_CONTROL_LEFT` |
| 57 | `ControlRight` | `GHOSTTY_KEY_CONTROL_RIGHT` |
| 58 | `Enter` | `GHOSTTY_KEY_ENTER` |
| 59 | `MetaLeft` | `GHOSTTY_KEY_META_LEFT` |
| 60 | `MetaRight` | `GHOSTTY_KEY_META_RIGHT` |
| 61 | `ShiftLeft` | `GHOSTTY_KEY_SHIFT_LEFT` |
| 62 | `ShiftRight` | `GHOSTTY_KEY_SHIFT_RIGHT` |
| 63 | `Space` | `GHOSTTY_KEY_SPACE` |
| 64 | `Tab` | `GHOSTTY_KEY_TAB` |
| 65 | `Convert` | `GHOSTTY_KEY_CONVERT` |
| 66 | `KanaMode` | `GHOSTTY_KEY_KANA_MODE` |
| 67 | `NonConvert` | `GHOSTTY_KEY_NON_CONVERT` |
| 68 | `Delete` | `GHOSTTY_KEY_DELETE` |
| 69 | `End` | `GHOSTTY_KEY_END` |
| 70 | `Help` | `GHOSTTY_KEY_HELP` |
| 71 | `Home` | `GHOSTTY_KEY_HOME` |
| 72 | `Insert` | `GHOSTTY_KEY_INSERT` |
| 73 | `PageDown` | `GHOSTTY_KEY_PAGE_DOWN` |
| 74 | `PageUp` | `GHOSTTY_KEY_PAGE_UP` |
| 75 | `ArrowDown` | `GHOSTTY_KEY_ARROW_DOWN` |
| 76 | `ArrowLeft` | `GHOSTTY_KEY_ARROW_LEFT` |
| 77 | `ArrowRight` | `GHOSTTY_KEY_ARROW_RIGHT` |
| 78 | `ArrowUp` | `GHOSTTY_KEY_ARROW_UP` |
| 79 | `NumLock` | `GHOSTTY_KEY_NUM_LOCK` |
| 80 | `Numpad0` | `GHOSTTY_KEY_NUMPAD_0` |
| 81 | `Numpad1` | `GHOSTTY_KEY_NUMPAD_1` |
| 82 | `Numpad2` | `GHOSTTY_KEY_NUMPAD_2` |
| 83 | `Numpad3` | `GHOSTTY_KEY_NUMPAD_3` |
| 84 | `Numpad4` | `GHOSTTY_KEY_NUMPAD_4` |
| 85 | `Numpad5` | `GHOSTTY_KEY_NUMPAD_5` |
| 86 | `Numpad6` | `GHOSTTY_KEY_NUMPAD_6` |
| 87 | `Numpad7` | `GHOSTTY_KEY_NUMPAD_7` |
| 88 | `Numpad8` | `GHOSTTY_KEY_NUMPAD_8` |
| 89 | `Numpad9` | `GHOSTTY_KEY_NUMPAD_9` |
| 90 | `NumpadAdd` | `GHOSTTY_KEY_NUMPAD_ADD` |
| 91 | `NumpadBackspace` | `GHOSTTY_KEY_NUMPAD_BACKSPACE` |
| 92 | `NumpadClear` | `GHOSTTY_KEY_NUMPAD_CLEAR` |
| 93 | `NumpadClearEntry` | `GHOSTTY_KEY_NUMPAD_CLEAR_ENTRY` |
| 94 | `NumpadComma` | `GHOSTTY_KEY_NUMPAD_COMMA` |
| 95 | `NumpadDecimal` | `GHOSTTY_KEY_NUMPAD_DECIMAL` |
| 96 | `NumpadDivide` | `GHOSTTY_KEY_NUMPAD_DIVIDE` |
| 97 | `NumpadEnter` | `GHOSTTY_KEY_NUMPAD_ENTER` |
| 98 | `NumpadEqual` | `GHOSTTY_KEY_NUMPAD_EQUAL` |
| 99 | `NumpadMemoryAdd` | `GHOSTTY_KEY_NUMPAD_MEMORY_ADD` |
| 100 | `NumpadMemoryClear` | `GHOSTTY_KEY_NUMPAD_MEMORY_CLEAR` |
| 101 | `NumpadMemoryRecall` | `GHOSTTY_KEY_NUMPAD_MEMORY_RECALL` |
| 102 | `NumpadMemoryStore` | `GHOSTTY_KEY_NUMPAD_MEMORY_STORE` |
| 103 | `NumpadMemorySubtract` | `GHOSTTY_KEY_NUMPAD_MEMORY_SUBTRACT` |
| 104 | `NumpadMultiply` | `GHOSTTY_KEY_NUMPAD_MULTIPLY` |
| 105 | `NumpadParenLeft` | `GHOSTTY_KEY_NUMPAD_PAREN_LEFT` |
| 106 | `NumpadParenRight` | `GHOSTTY_KEY_NUMPAD_PAREN_RIGHT` |
| 107 | `NumpadSubtract` | `GHOSTTY_KEY_NUMPAD_SUBTRACT` |
| 108 | `NumpadSeparator` | `GHOSTTY_KEY_NUMPAD_SEPARATOR` |
| 109 | `NumpadUp` | `GHOSTTY_KEY_NUMPAD_UP` |
| 110 | `NumpadDown` | `GHOSTTY_KEY_NUMPAD_DOWN` |
| 111 | `NumpadRight` | `GHOSTTY_KEY_NUMPAD_RIGHT` |
| 112 | `NumpadLeft` | `GHOSTTY_KEY_NUMPAD_LEFT` |
| 113 | `NumpadBegin` | `GHOSTTY_KEY_NUMPAD_BEGIN` |
| 114 | `NumpadHome` | `GHOSTTY_KEY_NUMPAD_HOME` |
| 115 | `NumpadEnd` | `GHOSTTY_KEY_NUMPAD_END` |
| 116 | `NumpadInsert` | `GHOSTTY_KEY_NUMPAD_INSERT` |
| 117 | `NumpadDelete` | `GHOSTTY_KEY_NUMPAD_DELETE` |
| 118 | `NumpadPageUp` | `GHOSTTY_KEY_NUMPAD_PAGE_UP` |
| 119 | `NumpadPageDown` | `GHOSTTY_KEY_NUMPAD_PAGE_DOWN` |
| 120 | `Escape` | `GHOSTTY_KEY_ESCAPE` |
| 121 | `F1` | `GHOSTTY_KEY_F1` |
| 122 | `F2` | `GHOSTTY_KEY_F2` |
| 123 | `F3` | `GHOSTTY_KEY_F3` |
| 124 | `F4` | `GHOSTTY_KEY_F4` |
| 125 | `F5` | `GHOSTTY_KEY_F5` |
| 126 | `F6` | `GHOSTTY_KEY_F6` |
| 127 | `F7` | `GHOSTTY_KEY_F7` |
| 128 | `F8` | `GHOSTTY_KEY_F8` |
| 129 | `F9` | `GHOSTTY_KEY_F9` |
| 130 | `F10` | `GHOSTTY_KEY_F10` |
| 131 | `F11` | `GHOSTTY_KEY_F11` |
| 132 | `F12` | `GHOSTTY_KEY_F12` |
| 133 | `F13` | `GHOSTTY_KEY_F13` |
| 134 | `F14` | `GHOSTTY_KEY_F14` |
| 135 | `F15` | `GHOSTTY_KEY_F15` |
| 136 | `F16` | `GHOSTTY_KEY_F16` |
| 137 | `F17` | `GHOSTTY_KEY_F17` |
| 138 | `F18` | `GHOSTTY_KEY_F18` |
| 139 | `F19` | `GHOSTTY_KEY_F19` |
| 140 | `F20` | `GHOSTTY_KEY_F20` |
| 141 | `F21` | `GHOSTTY_KEY_F21` |
| 142 | `F22` | `GHOSTTY_KEY_F22` |
| 143 | `F23` | `GHOSTTY_KEY_F23` |
| 144 | `F24` | `GHOSTTY_KEY_F24` |
| 145 | `F25` | `GHOSTTY_KEY_F25` |
| 146 | `Fn` | `GHOSTTY_KEY_FN` |
| 147 | `FnLock` | `GHOSTTY_KEY_FN_LOCK` |
| 148 | `PrintScreen` | `GHOSTTY_KEY_PRINT_SCREEN` |
| 149 | `ScrollLock` | `GHOSTTY_KEY_SCROLL_LOCK` |
| 150 | `Pause` | `GHOSTTY_KEY_PAUSE` |
| 151 | `BrowserBack` | `GHOSTTY_KEY_BROWSER_BACK` |
| 152 | `BrowserFavorites` | `GHOSTTY_KEY_BROWSER_FAVORITES` |
| 153 | `BrowserForward` | `GHOSTTY_KEY_BROWSER_FORWARD` |
| 154 | `BrowserHome` | `GHOSTTY_KEY_BROWSER_HOME` |
| 155 | `BrowserRefresh` | `GHOSTTY_KEY_BROWSER_REFRESH` |
| 156 | `BrowserSearch` | `GHOSTTY_KEY_BROWSER_SEARCH` |
| 157 | `BrowserStop` | `GHOSTTY_KEY_BROWSER_STOP` |
| 158 | `Eject` | `GHOSTTY_KEY_EJECT` |
| 159 | `LaunchApp1` | `GHOSTTY_KEY_LAUNCH_APP_1` |
| 160 | `LaunchApp2` | `GHOSTTY_KEY_LAUNCH_APP_2` |
| 161 | `LaunchMail` | `GHOSTTY_KEY_LAUNCH_MAIL` |
| 162 | `MediaPlayPause` | `GHOSTTY_KEY_MEDIA_PLAY_PAUSE` |
| 163 | `MediaSelect` | `GHOSTTY_KEY_MEDIA_SELECT` |
| 164 | `MediaStop` | `GHOSTTY_KEY_MEDIA_STOP` |
| 165 | `MediaTrackNext` | `GHOSTTY_KEY_MEDIA_TRACK_NEXT` |
| 166 | `MediaTrackPrevious` | `GHOSTTY_KEY_MEDIA_TRACK_PREVIOUS` |
| 167 | `Power` | `GHOSTTY_KEY_POWER` |
| 168 | `Sleep` | `GHOSTTY_KEY_SLEEP` |
| 169 | `AudioVolumeDown` | `GHOSTTY_KEY_AUDIO_VOLUME_DOWN` |
| 170 | `AudioVolumeMute` | `GHOSTTY_KEY_AUDIO_VOLUME_MUTE` |
| 171 | `AudioVolumeUp` | `GHOSTTY_KEY_AUDIO_VOLUME_UP` |
| 172 | `WakeUp` | `GHOSTTY_KEY_WAKE_UP` |
| 173 | `Copy` | `GHOSTTY_KEY_COPY` |
| 174 | `Cut` | `GHOSTTY_KEY_CUT` |
| 175 | `Paste` | `GHOSTTY_KEY_PASTE` |

## Approved current Character subset

The existing US-position subset consists of ASCII letters A through Z and a
through z, digits 0 through 9, and precisely the following characters:

```text
` \ [ ] , = - . ' ; / SPACE
```

That is 74 accepted character values naming 48 physical positions. Uppercase
letters are aliases for the same position, not evidence of a Shift modifier.
Shifted punctuation is not an alias. For example `!`, `?`, `+`, `~`, `|`, `:` and
`"` do not identify a supported current Character position. Neither do Tab,
Return, DEL, non-ASCII characters or an empty/multi-character value.

Manager must reject those unsupported Character values instead of obtaining
physical zero. Use named Tab/Enter/etc. for those physical keys. This does not
ban explicit `Unidentified` plus logical text when physical identity is genuinely
unavailable, including IME or committed-text delivery. Old V1/Observer shapes
remain unchanged. The new KeyV2 model has named physical variants, no Character
variant and no text-to-physical inference.
