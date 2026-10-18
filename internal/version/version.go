// Package version reports the moxi build identity.
package version

import (
	"runtime/debug"
	"strings"
	"time"
)

// Version is the release label until a tagged release exists.
const Version = "0.0.0"

// Line is the binary name, version, commit, and commit time when the build recorded them.
func Line() string {
	line := "moxi " + Version
	info, ok := debug.ReadBuildInfo()
	if !ok {
		return line
	}
	settings := map[string]string{}
	for _, s := range info.Settings {
		settings[s.Key] = s.Value
	}
	if rev := settings["vcs.revision"]; rev != "" {
		if len(rev) > 12 {
			rev = rev[:12]
		}
		line += "  commit " + rev
		if settings["vcs.modified"] == "true" {
			line += " (local edits)"
		}
	}
	if at, err := time.Parse(time.RFC3339, settings["vcs.time"]); err == nil {
		line += "  committed " + at.Local().Format("2006-01-02 15:04")
	}
	return strings.TrimSpace(line)
}
