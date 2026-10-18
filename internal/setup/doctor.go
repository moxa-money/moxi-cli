// Package setup is the founder machine check and the editor scaffold.
package setup

import (
	"context"
	"fmt"
	"io"
	"net/http"
	"os/exec"
	"strings"
	"time"
)

const (
	awsInstallURL     = "https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html"
	gcloudInstallURL  = "https://cloud.google.com/sdk/docs/install"
	workspaceAdminURL = "https://admin.google.com"
	workspaceScopes   = "openid,https://www.googleapis.com/auth/admin.directory.user.readonly"
)

// workspaceUsersURL is the directory read used to confirm Workspace admin access.
var workspaceUsersURL = "https://admin.googleapis.com/admin/directory/v1/users?customer=my_customer&maxResults=1"

// Check is one doctor row. Detail is a fixed remediation. It is not command output.
type Check struct {
	Name   string `json:"name"`
	OK     bool   `json:"ok"`
	Detail string `json:"detail"`
}

// Report is the doctor result. Checks stop at the first failure.
type Report struct {
	OK     bool    `json:"ok"`
	Checks []Check `json:"checks"`
}

// Env is how doctor sees the machine. Run must not put command output in its error.
type Env struct {
	LookPath  func(string) error
	Run       func(context.Context, string, ...string) (string, error)
	Workspace func(context.Context, string) (int, error)
}

// LiveEnv uses the local PATH, the local CLIs, and the Workspace directory API.
func LiveEnv() Env {
	return Env{
		LookPath:  lookPath,
		Run:       Run,
		Workspace: WorkspaceStatus,
	}
}

func lookPath(file string) error {
	_, err := exec.LookPath(file)
	return err
}

// Run executes name and returns stdout. Stderr is discarded. A non-zero exit
// returns an error that does not include either stream.
func Run(ctx context.Context, name string, args ...string) (string, error) {
	cmd := exec.CommandContext(ctx, name, args...)
	var stdout strings.Builder
	cmd.Stdout = &stdout
	cmd.Stderr = io.Discard
	if err := cmd.Run(); err != nil {
		return "", err
	}
	return stdout.String(), nil
}

// WorkspaceStatus calls the Workspace directory API and returns the HTTP status.
// The body is discarded. token is not written anywhere.
func WorkspaceStatus(ctx context.Context, token string) (int, error) {
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, workspaceUsersURL, nil)
	if err != nil {
		return 0, err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	client := &http.Client{Timeout: 20 * time.Second}
	resp, err := client.Do(req)
	if err != nil {
		return 0, err
	}
	defer resp.Body.Close()
	_, _ = io.Copy(io.Discard, resp.Body)
	return resp.StatusCode, nil
}

// Doctor checks the config file and cloud sign-in. It stops at the first failure
// so the founder sees one remediation. Command output is never copied into a detail.
func Doctor(ctx context.Context, configPath string, configExists bool, env Env) Report {
	if env.LookPath == nil || env.Run == nil || env.Workspace == nil {
		env = LiveEnv()
	}
	rep := Report{}
	add := func(c Check) bool {
		rep.Checks = append(rep.Checks, c)
		return c.OK
	}
	if !add(Check{Name: "go-runtime", OK: true, Detail: goVersion()}) {
		return rep
	}
	if !configExists {
		if !add(Check{Name: "config-file", OK: false, Detail: "missing — run: moxi config init"}) {
			return rep
		}
	} else if !add(Check{Name: "config-file", OK: true, Detail: configPath}) {
		return rep
	}
	if !add(cliCheck(env, "aws-cli", "aws", "not on PATH — install: "+awsInstallURL)) {
		return rep
	}
	if !add(authCheck(ctx, env, "aws-auth", "aws", []string{"sts", "get-caller-identity"}, "not signed in — run: aws login")) {
		return rep
	}
	if !add(cliCheck(env, "gcloud", "gcloud", "not on PATH — install: "+gcloudInstallURL)) {
		return rep
	}
	if !add(authCheck(ctx, env, "gcp-auth", "gcloud", []string{"auth", "print-access-token"}, "not signed in — run: gcloud auth login")) {
		return rep
	}
	if !add(workspaceCheck(ctx, env)) {
		return rep
	}
	rep.OK = true
	return rep
}

func cliCheck(env Env, name, bin, miss string) Check {
	if err := env.LookPath(bin); err != nil {
		return Check{Name: name, OK: false, Detail: miss}
	}
	return Check{Name: name, OK: true, Detail: "on PATH"}
}

func authCheck(ctx context.Context, env Env, name, bin string, args []string, miss string) Check {
	if _, err := env.Run(ctx, bin, args...); err != nil {
		return Check{Name: name, OK: false, Detail: miss}
	}
	return Check{Name: name, OK: true, Detail: "signed in"}
}

func workspaceCheck(ctx context.Context, env Env) Check {
	token, err := env.Run(ctx, "gcloud", "auth", "print-access-token")
	token = strings.TrimSpace(token)
	if err != nil || token == "" {
		return Check{Name: "workspace-auth", OK: false, Detail: "not signed in — run: gcloud auth login"}
	}
	code, err := env.Workspace(ctx, token)
	if err != nil {
		return Check{Name: "workspace-auth", OK: false, Detail: "could not reach admin.googleapis.com"}
	}
	if code != http.StatusOK {
		return Check{
			Name:   "workspace-auth",
			OK:     false,
			Detail: fmt.Sprintf("directory access was refused (HTTP %d) — open %s as an admin, then run: gcloud auth login --scopes=%s", code, workspaceAdminURL, workspaceScopes),
		}
	}
	return Check{Name: "workspace-auth", OK: true, Detail: "directory access confirmed"}
}
