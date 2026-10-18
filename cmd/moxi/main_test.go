package main

import (
	"bytes"
	"strings"
	"testing"

	"github.com/moxa-money/moxi-cli/internal/cli"
	"github.com/moxa-money/moxi-cli/internal/configroot"
)

func TestVersionCommand(t *testing.T) {
	t.Setenv(configroot.EnvDir, t.TempDir())
	var stdout, stderr bytes.Buffer
	if err := cli.Execute([]string{"version"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(stdout.String(), "moxi ") {
		t.Fatalf("stdout = %q", stdout.String())
	}
}
