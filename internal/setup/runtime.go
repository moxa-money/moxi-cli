package setup

import "runtime"

func goVersion() string {
	return runtime.Version()
}
