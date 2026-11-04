package inner

type Base struct{}

func (b Base) Hello() string {
	Plain()
	return ""
}

func Plain() int { return 1 }
