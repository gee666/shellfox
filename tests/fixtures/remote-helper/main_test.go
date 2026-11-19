package main

import (
	"context"
	"net"
	"testing"
)

func TestFixtureDialsOnlyIPv4Loopback(t *testing.T) {
	for _, address := range []string{"", "localhost:80", "[::1]:80", "127.0.0.2:80", "192.0.2.1:80"} {
		if conn, err := dialFixture(context.Background(), "tcp", address); err == nil {
			conn.Close()
			t.Fatalf("allowed non-fixture destination %q", address)
		}
	}
	listener, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	conn, err := dialFixture(context.Background(), "tcp", listener.Addr().String())
	if err != nil {
		t.Fatal(err)
	}
	conn.Close()
}
