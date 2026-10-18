package main

import (
	"bytes"
	"strings"
	"testing"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

func TestRunName(t *testing.T) {
	var buf bytes.Buffer
	if err := run(nil, &buf); err != nil {
		t.Fatal(err)
	}
	if buf.String() != "moxi\n" {
		t.Fatalf("stdout = %q", buf.String())
	}
}

func TestRunConfigRoot(t *testing.T) {
	want := t.TempDir()
	t.Setenv(configroot.EnvDir, want)
	var buf bytes.Buffer
	if err := run([]string{"config-root"}, &buf); err != nil {
		t.Fatal(err)
	}
	if strings.TrimSpace(buf.String()) != want {
		t.Fatalf("stdout = %q, want %q", buf.String(), want)
	}
}

func TestRunUnknown(t *testing.T) {
	if err := run([]string{"help"}, ioDiscard{}); err == nil {
		t.Fatal("expected an error")
	}
}

type ioDiscard struct{}

func (ioDiscard) Write(p []byte) (int, error) { return len(p), nil }
