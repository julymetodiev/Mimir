//go:build withdeploy

package fixture

// A file behind a build tag is in no index, and keeps syntax edges.
func deployed() int { return helper() }

func dual() int { return 1 }
