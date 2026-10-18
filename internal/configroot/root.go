// Package configroot is the founder config directory for the moxi binary.
package configroot

import (
	"fmt"
	"os"
	"path/filepath"
)

const (
	// DirName is the directory under the home directory.
	DirName = ".moxi"
	// EnvDir overrides the config directory. Tests and a founder who wants another path set it.
	EnvDir = "MOXI_CONFIG_DIR"
)

// Dir returns the founder config directory.
// MOXI_CONFIG_DIR wins when it is set. Otherwise the path is ~/.moxi.
func Dir() (string, error) {
	if v := os.Getenv(EnvDir); v != "" {
		return v, nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("founder config root: %w", err)
	}
	return filepath.Join(home, DirName), nil
}
