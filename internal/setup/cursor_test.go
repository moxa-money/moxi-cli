package setup

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestCursorScaffoldIsSanitized(t *testing.T) {
	dir := t.TempDir()
	var out bytes.Buffer
	if err := Cursor(dir, &out); err != nil {
		t.Fatal(err)
	}
	rules, err := os.ReadFile(filepath.Join(dir, ".cursor", "rules", "founder.mdc"))
	if err != nil {
		t.Fatal(err)
	}
	example, err := os.ReadFile(filepath.Join(dir, ".cursor", "mcp.json.example"))
	if err != nil {
		t.Fatal(err)
	}
	body := string(rules) + string(example) + out.String()
	for _, banned := range []string{
		"moxa-cli",
		"~/.moxa",
		"moxa-dev",
		"github.com/moxa-money/moxa-cli",
		"control-plane",
		"control_plane",
		"AKIA",
	} {
		if strings.Contains(body, banned) {
			t.Fatalf("scaffold contains %s\n%s", banned, body)
		}
	}
	if !strings.Contains(string(example), `"mcpServers": {}`) {
		t.Fatalf("example = %s", example)
	}
	out.Reset()
	if err := Cursor(dir, &out); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out.String(), "skip (exists)") {
		t.Fatalf("second scaffold = %s", out.String())
	}
}
