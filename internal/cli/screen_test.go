package cli

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

func TestDeskAndDashAndConsent(t *testing.T) {
	dir := t.TempDir()
	t.Setenv(configroot.EnvDir, dir)
	t.Setenv("MOXI_LEDGER_DIR", filepath.Join(dir, "ledger"))
	if err := os.MkdirAll(filepath.Join(dir, "ledger", "data"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "ledger", "owners.toml"), []byte("example-app = APP\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "ledger", "data", "events.jsonl"), []byte("{\"op\":\"plan_open\",\"plan_ref\":\"APP-1\",\"summary\":\"Open the shell\"}\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	var stdout, stderr bytes.Buffer
	if err := Execute([]string{"desk"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	text := stdout.String()
	for _, word := range []string{"status", "activity", "Open the shell", "cursor", "link"} {
		if !strings.Contains(text, word) {
			t.Fatalf("desk missing %s\n%s", word, text)
		}
	}

	stdout.Reset()
	page := filepath.Join(dir, "dash.html")
	if err := Execute([]string{"dash", "--out", page}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	body, err := os.ReadFile(page)
	if err != nil {
		t.Fatal(err)
	}
	html := string(body)
	if !strings.Contains(html, "Desk") || !strings.Contains(html, "Open the shell") || !strings.Contains(html, "max-width: 894px") {
		t.Fatalf("dash page = %s", html)
	}

	plan := filepath.Join(dir, "plan.txt")
	cost := filepath.Join(dir, "cost.txt")
	if err := os.WriteFile(plan, []byte("closed network\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(cost, []byte("monthly  $0\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	stdout.Reset()
	if err := Execute([]string{"setup", "consent", "--plan", plan, "--cost", cost, "--output", "json"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(stdout.String(), `"status": "canonical"`) || !strings.Contains(stdout.String(), `"digest"`) {
		t.Fatalf("consent = %s", stdout.String())
	}
}

func TestCommsForwards(t *testing.T) {
	dir := t.TempDir()
	t.Setenv(configroot.EnvDir, dir)
	bin := filepath.Join(dir, "comms-stub.sh")
	script := "#!/bin/sh\nprintf '%s\\n' \"$*\"\n"
	if err := os.WriteFile(bin, []byte(script), 0o700); err != nil {
		t.Fatal(err)
	}
	t.Setenv("MOXI_COMMS_BIN", bin)
	var stdout, stderr bytes.Buffer
	if err := Execute([]string{"comms", "ready", "--json"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(stdout.String(), "ready --json") {
		t.Fatalf("stdout = %s", stdout.String())
	}
}
