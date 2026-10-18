package configroot

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestDirOverride(t *testing.T) {
	want := t.TempDir()
	t.Setenv(EnvDir, want)
	got, err := Dir()
	if err != nil {
		t.Fatal(err)
	}
	if got != want {
		t.Fatalf("Dir() = %q, want %q", got, want)
	}
}

func TestDirDefault(t *testing.T) {
	t.Setenv(EnvDir, "")
	got, err := Dir()
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasSuffix(got, string(filepath.Separator)+DirName) {
		t.Fatalf("Dir() = %q, want a path ending in %s", got, DirName)
	}
}

func TestModulePath(t *testing.T) {
	body, err := os.ReadFile(filepath.Join("..", "..", "go.mod"))
	if err != nil {
		t.Fatal(err)
	}
	line := strings.SplitN(string(body), "\n", 2)[0]
	const want = "module github.com/moxa-money/moxi-cli"
	if line != want {
		t.Fatalf("go.mod = %q, want %q", line, want)
	}
}
