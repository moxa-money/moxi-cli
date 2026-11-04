package cli

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/spf13/cobra"

	"github.com/moxa-money/moxi-cli/internal/configroot"
	"github.com/moxa-money/moxi-cli/internal/output"
)

// DeskView is the local screen: status line, activity list, and command cursor.
type DeskView struct {
	Status   string   `json:"status"`
	Activity []string `json:"activity"`
	Cursor   string   `json:"cursor"`
	Link     string   `json:"link"`
	Workers  []string `json:"workers"`
}

func newDeskCmd(st *State) *cobra.Command {
	return &cobra.Command{
		Use:   "desk",
		Short: "Status line, activity list, and command cursor",
		RunE: func(cmd *cobra.Command, _ []string) error {
			view, err := loadDesk()
			if err != nil {
				return err
			}
			if st.JSON() {
				return output.JSON(cmd.OutOrStdout(), view)
			}
			fmt.Fprintf(cmd.OutOrStdout(), "status    %s\n", view.Status)
			if len(view.Activity) == 0 {
				fmt.Fprintln(cmd.OutOrStdout(), "activity  none")
			} else {
				fmt.Fprintf(cmd.OutOrStdout(), "activity  %s\n", view.Activity[0])
				for _, line := range view.Activity[1:] {
					fmt.Fprintf(cmd.OutOrStdout(), "          %s\n", line)
				}
			}
			fmt.Fprintf(cmd.OutOrStdout(), "cursor    %s\n", view.Cursor)
			fmt.Fprintf(cmd.OutOrStdout(), "link      %s\n", view.Link)
			return nil
		},
	}
}

func newDashCmd(st *State) *cobra.Command {
	var out string
	cmd := &cobra.Command{
		Use:   "dash",
		Short: "Write the local desk page",
		RunE: func(cmd *cobra.Command, _ []string) error {
			view, err := loadDesk()
			if err != nil {
				return err
			}
			path := out
			if path == "" {
				dir, err := configroot.Dir()
				if err != nil {
					return err
				}
				if err := os.MkdirAll(dir, 0o700); err != nil {
					return err
				}
				path = filepath.Join(dir, "dash.html")
			}
			if err := os.WriteFile(path, []byte(dashPage(view)), 0o600); err != nil {
				return err
			}
			if st.JSON() {
				return output.JSON(cmd.OutOrStdout(), map[string]string{"path": path, "status": view.Status})
			}
			fmt.Fprintln(cmd.OutOrStdout(), path)
			return nil
		},
	}
	cmd.Flags().StringVar(&out, "out", "", "page path (default is dash.html in the config directory)")
	return cmd
}

func newCommsCmd() *cobra.Command {
	cmd := &cobra.Command{
		Use:                "comms",
		Short:              "Run the local ledger",
		DisableFlagParsing: true,
		RunE: func(cmd *cobra.Command, args []string) error {
			bin, err := commsBinary()
			if err != nil {
				return err
			}
			if len(args) == 0 {
				args = []string{"--help"}
			}
			child := exec.Command(bin, args...)
			child.Stdout = cmd.OutOrStdout()
			child.Stderr = cmd.ErrOrStderr()
			child.Stdin = os.Stdin
			return child.Run()
		},
	}
	return cmd
}

func commsBinary() (string, error) {
	if bin := os.Getenv("MOXI_COMMS_BIN"); bin != "" {
		return bin, nil
	}
	if path, err := exec.LookPath("comms"); err == nil {
		return path, nil
	}
	return "", fmt.Errorf("comms is not built. From this repo: cargo build --release --manifest-path ledger/Cargo.toml")
}

func loadDesk() (DeskView, error) {
	dir, err := ledgerDir()
	if err != nil {
		return DeskView{}, err
	}
	view := DeskView{
		Status:  "ledger empty",
		Cursor:  "moxi desk",
		Link:    "local",
		Workers: readWorkers(filepath.Join(dir, "owners.toml")),
	}
	logPath := filepath.Join(dir, "data", "events.jsonl")
	lines, err := activityLines(logPath)
	if err != nil {
		return DeskView{}, err
	}
	if len(lines) == 0 {
		if _, err := os.Stat(logPath); err == nil {
			view.Status = "ledger open"
		}
		return view, nil
	}
	view.Status = fmt.Sprintf("%d events", len(lines))
	view.Activity = lines
	return view, nil
}

func ledgerDir() (string, error) {
	if dir := os.Getenv("MOXI_LEDGER_DIR"); dir != "" {
		return dir, nil
	}
	root, err := configroot.Dir()
	if err != nil {
		return "", err
	}
	return filepath.Join(root, "ledger"), nil
}

func readWorkers(path string) []string {
	body, err := os.ReadFile(path)
	if err != nil {
		return nil
	}
	var names []string
	for _, line := range strings.Split(string(body), "\n") {
		line = strings.TrimSpace(line)
		if line == "" || strings.HasPrefix(line, "#") || !strings.Contains(line, "=") {
			continue
		}
		parts := strings.SplitN(line, "=", 2)
		name := strings.TrimSpace(parts[1])
		if name != "" {
			names = append(names, name)
		}
	}
	return names
}

func activityLines(path string) ([]string, error) {
	f, err := os.Open(path)
	if err != nil {
		if os.IsNotExist(err) {
			return nil, nil
		}
		return nil, err
	}
	defer f.Close()
	var lines []string
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 0, 64*1024), 1024*1024)
	for sc.Scan() {
		text := strings.TrimSpace(sc.Text())
		if text == "" {
			continue
		}
		var row map[string]any
		if err := json.Unmarshal([]byte(text), &row); err != nil {
			continue
		}
		line := activityLine(row)
		if line == "" {
			continue
		}
		lines = append(lines, line)
		if len(lines) == 8 {
			break
		}
	}
	return lines, sc.Err()
}

func activityLine(row map[string]any) string {
	if summary, ok := row["summary"].(string); ok && summary != "" {
		return summary
	}
	if op, ok := row["op"].(string); ok && op != "" {
		if ref, ok := row["plan_ref"].(string); ok && ref != "" {
			return op + " " + ref
		}
		return op
	}
	return ""
}

func dashPage(view DeskView) string {
	var activity strings.Builder
	if len(view.Activity) == 0 {
		activity.WriteString("<p>none</p>")
	} else {
		activity.WriteString("<ul>")
		for _, line := range view.Activity {
			activity.WriteString("<li>")
			activity.WriteString(htmlText(line))
			activity.WriteString("</li>")
		}
		activity.WriteString("</ul>")
	}
	var workers strings.Builder
	if len(view.Workers) == 0 {
		workers.WriteString("none named")
	} else {
		workers.WriteString(htmlText(strings.Join(view.Workers, ", ")))
	}
	return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Desk</title>
<style>
body { margin: 0; font: 16px/1.4 system-ui, sans-serif; color: #1a1814; background: #fff; }
main { max-width: 894px; margin: 0 auto; padding: 24px 16px 48px; }
h1 { font-size: 28px; margin: 0 0 8px; }
p.lead { margin: 0 0 24px; }
section { margin: 0 0 20px; }
h2 { font-size: 14px; letter-spacing: 0.04em; text-transform: uppercase; margin: 0 0 8px; }
ul { margin: 0; padding-left: 18px; }
</style>
</head>
<body>
<main>
<h1>Desk</h1>
<p class="lead">` + htmlText(view.Status) + `</p>
<section><h2>Activity</h2>` + activity.String() + `</section>
<section><h2>Workers</h2><p>` + workers.String() + `</p></section>
<section><h2>Cursor</h2><p>` + htmlText(view.Cursor) + `</p></section>
<section><h2>Link</h2><p>` + htmlText(view.Link) + `</p></section>
</main>
</body>
</html>
`
}

func htmlText(s string) string {
	r := strings.NewReplacer("&", "&amp;", "<", "&lt;", ">", "&gt;", `"`, "&quot;")
	return r.Replace(s)
}
