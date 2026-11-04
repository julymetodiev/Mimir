package fixture

import "testing"

func TestDirect(t *testing.T) {
	if direct() != 1 {
		t.Fail()
	}
}
