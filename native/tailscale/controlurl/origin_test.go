package controlurl

import "testing"

func TestParseLoopback(t *testing.T) {
	for input, want := range map[string]string{
		"http://127.0.0.1:1234":  "http://127.0.0.1:1234",
		"http://127.0.0.1:1234/": "http://127.0.0.1:1234",
		"http://[::1]:1234":      "http://[::1]:1234",
	} {
		got, err := ParseLoopback(input)
		if err != nil || got != want {
			t.Fatal("literal control origin failed")
		}
	}
	for _, input := range []string{"", "https://127.0.0.1:1234", "http://localhost:1234", "http://192.0.2.1:1234", "http://127.0.0.1:0", "http://127.0.0.1:65536", "http://127.0.0.1", "http://user:password@127.0.0.1:1234", "http://127.0.0.1:1234/path", "http://127.0.0.1:1234?", "http://127.0.0.1:1234#", "http://[::ffff:127.0.0.1]:1234", "http://[::1%25lo]:1234", "relative", "\nhttp://127.0.0.1:1234"} {
		if _, err := ParseLoopback(input); err == nil {
			t.Fatal("unsafe control origin accepted")
		}
	}
}
