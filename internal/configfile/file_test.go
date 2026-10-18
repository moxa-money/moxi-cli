package configfile

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

func TestInitSetGet(t *testing.T) {
	dir := t.TempDir()
	t.Setenv(configroot.EnvDir, dir)

	path, err := Init()
	if err != nil {
		t.Fatal(err)
	}
	if path != filepath.Join(dir, "config.yaml") {
		t.Fatalf("path = %s", path)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o600 {
		t.Fatalf("mode = %o", info.Mode().Perm())
	}
	if _, err := Init(); err == nil {
		t.Fatal("second init should fail")
	}

	f, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if err := f.Set("output", "json"); err != nil {
		t.Fatal(err)
	}
	if err := f.Set("output", "yaml"); err == nil {
		t.Fatal("yaml output should fail")
	}
	if err := f.Set("oidc.issuer", "https://example"); err == nil {
		t.Fatal("unknown key should fail")
	}
	got, err := f.Get("output")
	if err != nil || got != "json" {
		t.Fatalf("output = %q, %v", got, err)
	}
}

func TestProfileAndContext(t *testing.T) {
	t.Setenv(configroot.EnvDir, t.TempDir())
	if _, err := Init(); err != nil {
		t.Fatal(err)
	}
	f, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if err := f.CreateProfile("Dev"); err == nil {
		t.Fatal("mixed-case profile should fail")
	}
	if err := f.CreateProfile("studio"); err != nil {
		t.Fatal(err)
	}
	if err := f.UseProfile("studio"); err != nil {
		t.Fatal(err)
	}
	if err := f.Set("context", "lab"); err != nil {
		t.Fatal(err)
	}
	again, err := Load()
	if err != nil {
		t.Fatal(err)
	}
	if again.Profile != "studio" || again.Context != "lab" {
		t.Fatalf("file = %+v", again)
	}
}
