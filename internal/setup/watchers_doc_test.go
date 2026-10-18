package setup

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestWatchersSetupDoc(t *testing.T) {
	path := filepath.Join("..", "..", "docs", "setup", "moxa-watchers.md")
	body, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	text := string(body)
	for _, want := range []string{
		"Moxa Watchers",
		"Self Signed Root",
		"Code Signing",
		"3650 days",
		"RSA",
		"2048",
		"login",
		"/usr/bin/codesign",
		"Always Allow",
		"security find-identity -v -p codesigning",
		"security export",
		"Key Usage Extension, Extended Key Usage Extension, and Basic Constraints Extension are not specified",
		"change nothing",
	} {
		if !strings.Contains(text, want) {
			t.Errorf("docs/setup/moxa-watchers.md missing %q", want)
		}
	}
	for _, banned := range []string{
		"Digital Signature",
		"Key Encipherment",
		"certificate authority",
		"This extension is critical",
		"~/.moxa",
		"moxa-cli",
	} {
		if strings.Contains(text, banned) {
			t.Errorf("docs/setup/moxa-watchers.md sets or names %q", banned)
		}
	}
}
