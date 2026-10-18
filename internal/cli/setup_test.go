package cli

import (
	"bytes"
	"context"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/moxa-money/moxi-cli/internal/configroot"
	"github.com/moxa-money/moxi-cli/internal/setup"
)

func TestSetupCursorEnable(t *testing.T) {
	conf := t.TempDir()
	dir := t.TempDir()
	t.Setenv(configroot.EnvDir, conf)
	var stdout, stderr bytes.Buffer
	if err := Execute([]string{"setup", "cursor", "--enable", dir}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
	if _, err := os.Stat(filepath.Join(dir, ".cursor", "rules", "founder.mdc")); err != nil {
		t.Fatal(err)
	}
	body, err := os.ReadFile(filepath.Join(conf, "config.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(body), "enabled: true") {
		t.Fatalf("config = %s", body)
	}
	stdout.Reset()
	if err := Execute([]string{"setup", "cursor", dir}, &stdout, &stderr); err != nil {
		t.Fatal(err)
	}
}

func TestSetupCursorDisabled(t *testing.T) {
	t.Setenv(configroot.EnvDir, t.TempDir())
	var stdout, stderr bytes.Buffer
	err := Execute([]string{"setup", "cursor", t.TempDir()}, &stdout, &stderr)
	if err == nil || !strings.Contains(err.Error(), "moxi setup cursor --enable") {
		t.Fatalf("err = %v", err)
	}
}

func TestSetupDoctorJSONStopsAndHidesSecrets(t *testing.T) {
	t.Setenv(configroot.EnvDir, t.TempDir())
	const token = "ya29.SUPERSECRET"
	const account = "account-sentinel"
	prev := doctorEnv
	t.Cleanup(func() { doctorEnv = prev })
	doctorEnv = func() setup.Env {
		return setup.Env{
			LookPath: func(string) error { return nil },
			Run: func(_ context.Context, name string, _ ...string) (string, error) {
				if name == "aws" {
					return account, nil
				}
				return token, nil
			},
			Workspace: func(context.Context, string) (int, error) {
				return http.StatusForbidden, nil
			},
		}
	}
	var stdout, stderr bytes.Buffer
	err := Execute([]string{"config", "init"}, &stdout, &stderr)
	if err != nil {
		t.Fatal(err)
	}
	stdout.Reset()
	err = Execute([]string{"setup", "doctor", "--output", "json"}, &stdout, &stderr)
	if err == nil {
		t.Fatal("expected doctor to fail")
	}
	if strings.Contains(stdout.String(), token) || strings.Contains(stdout.String(), account) {
		t.Fatalf("stdout leaked\n%s", stdout.String())
	}
	if strings.Contains(stderr.String(), token) {
		t.Fatalf("stderr leaked\n%s", stderr.String())
	}
	if !strings.Contains(stdout.String(), "workspace-auth") || !strings.Contains(stdout.String(), "admin.google.com") {
		t.Fatalf("stdout = %s", stdout.String())
	}
}
