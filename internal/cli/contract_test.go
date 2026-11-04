package cli

import (
	"bytes"
	"strings"
	"testing"

	"github.com/moxa-money/moxi-cli/internal/configroot"
)

func TestHelpTreeIsLocalContract(t *testing.T) {
	t.Setenv(configroot.EnvDir, t.TempDir())
	var stdout, stderr bytes.Buffer
	if err := Execute([]string{"help", "tree"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	tree := stdout.String()
	names := commandNames(tree)
	for _, name := range []string{"config", "ctx", "version", "usage", "help", "tree", "setup", "doctor", "cursor", "consent", "comms", "desk", "dash"} {
		if !names[name] {
			t.Fatalf("tree missing %s\n%s", name, tree)
		}
	}
	for _, banned := range []string{"login", "signer", "ui", "rapid", "ping", "directory", "health", "cost", "logs", "graph"} {
		if names[banned] {
			t.Fatalf("tree contains %s\n%s", banned, tree)
		}
	}
}

func commandNames(tree string) map[string]bool {
	names := map[string]bool{}
	for _, line := range strings.Split(tree, "\n") {
		line = strings.TrimLeft(line, " ├└│─")
		fields := strings.Fields(line)
		if len(fields) == 0 {
			continue
		}
		names[fields[0]] = true
	}
	return names
}

func TestEnvFlagRefused(t *testing.T) {
	t.Setenv(configroot.EnvDir, t.TempDir())
	var stdout, stderr bytes.Buffer
	if err := Execute([]string{"--env", "dev", "version"}, &stdout, &stderr); err == nil {
		t.Fatal("expected --env to fail")
	}
}

func TestConfigAndContext(t *testing.T) {
	dir := t.TempDir()
	t.Setenv(configroot.EnvDir, dir)

	var stdout, stderr bytes.Buffer
	if err := Execute([]string{"config", "init"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	stdout.Reset()
	if err := Execute([]string{"config", "set", "context", "lab"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	stdout.Reset()
	if err := Execute([]string{"ctx", "show", "--output", "json"}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(stdout.String(), `"context": "lab"`) {
		t.Fatalf("stdout = %s", stdout.String())
	}
	if strings.Contains(stdout.String(), "control_plane") {
		t.Fatalf("context show leaked a control plane field\n%s", stdout.String())
	}
}
