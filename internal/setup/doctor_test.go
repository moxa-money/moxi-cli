package setup

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
)

func TestDoctorStopsAtFirstMiss(t *testing.T) {
	var runs int
	env := Env{
		LookPath: func(name string) error {
			if name == "aws" {
				return errors.New("missing")
			}
			return nil
		},
		Run: func(context.Context, string, ...string) (string, error) {
			runs++
			return "secret-token", nil
		},
		Workspace: func(context.Context, string) (int, error) {
			t.Fatal("workspace called")
			return 0, nil
		},
	}
	rep := Doctor(context.Background(), "/tmp/config.yaml", true, env)
	if rep.OK {
		t.Fatal("expected failure")
	}
	if runs != 0 {
		t.Fatalf("runs = %d", runs)
	}
	last := rep.Checks[len(rep.Checks)-1]
	if last.Name != "aws-cli" || last.OK {
		t.Fatalf("last = %+v", last)
	}
	if !strings.Contains(last.Detail, awsInstallURL) {
		t.Fatalf("detail = %s", last.Detail)
	}
	for _, c := range rep.Checks {
		if c.Name == "gcp-auth" || c.Name == "workspace-auth" {
			t.Fatalf("continued past aws-cli: %+v", rep.Checks)
		}
	}
}

func TestDoctorHidesCommandOutput(t *testing.T) {
	const token = "ya29.SUPERSECRET"
	const account = "account-sentinel"
	env := Env{
		LookPath: func(string) error { return nil },
		Run: func(_ context.Context, name string, args ...string) (string, error) {
			if name == "aws" {
				return account, nil
			}
			return token, nil
		},
		Workspace: func(_ context.Context, got string) (int, error) {
			if got != token {
				t.Fatalf("token = %q", got)
			}
			return http.StatusForbidden, nil
		},
	}
	rep := Doctor(context.Background(), "/tmp/config.yaml", true, env)
	if rep.OK {
		t.Fatal("expected workspace failure")
	}
	text := reportText(rep)
	if strings.Contains(text, token) || strings.Contains(text, account) {
		t.Fatalf("report leaked command output\n%s", text)
	}
	last := rep.Checks[len(rep.Checks)-1]
	if last.Name != "workspace-auth" || !strings.Contains(last.Detail, workspaceAdminURL) {
		t.Fatalf("last = %+v", last)
	}
}

func TestDoctorAllPass(t *testing.T) {
	env := Env{
		LookPath: func(string) error { return nil },
		Run:      func(context.Context, string, ...string) (string, error) { return "tok", nil },
		Workspace: func(context.Context, string) (int, error) {
			return http.StatusOK, nil
		},
	}
	rep := Doctor(context.Background(), "/tmp/config.yaml", true, env)
	if !rep.OK {
		t.Fatalf("report = %+v", rep)
	}
	if len(rep.Checks) != 7 {
		t.Fatalf("checks = %+v", rep.Checks)
	}
}

func TestDoctorMissingConfigStops(t *testing.T) {
	env := Env{
		LookPath: func(string) error { return nil },
		Run: func(context.Context, string, ...string) (string, error) {
			t.Fatal("run called")
			return "", nil
		},
		Workspace: func(context.Context, string) (int, error) { return 0, nil },
	}
	rep := Doctor(context.Background(), "", false, env)
	if rep.OK || rep.Checks[len(rep.Checks)-1].Name != "config-file" {
		t.Fatalf("report = %+v", rep)
	}
}

func TestWorkspaceStatusDiscardsBody(t *testing.T) {
	const token = "ya29.BODYTOKEN"
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer "+token {
			t.Fatalf("auth = %s", r.Header.Get("Authorization"))
		}
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"users":[{"primaryEmail":"founder@example.com"}]}`))
	}))
	defer srv.Close()
	prev := workspaceUsersURL
	workspaceUsersURL = srv.URL
	t.Cleanup(func() { workspaceUsersURL = prev })

	code, err := WorkspaceStatus(context.Background(), token)
	if err != nil {
		t.Fatal(err)
	}
	if code != http.StatusOK {
		t.Fatalf("status = %d", code)
	}
}

func reportText(rep Report) string {
	var b strings.Builder
	for _, c := range rep.Checks {
		b.WriteString(c.Name)
		b.WriteString(c.Detail)
	}
	return b.String()
}
