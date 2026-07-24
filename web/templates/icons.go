package templates

import (
	"embed"
	"fmt"
	"strings"
	"sync"
)

//go:embed icons
var iconFS embed.FS

//go:embed illustrations
var illustrationFS embed.FS

// staticDefines returns every embedded icon and illustration file wrapped in a
// top-level {{define "icons/<name>"}} / {{define "illustrations/<name>"}} block.
var staticDefines = sync.OnceValue(func() string {
	var b strings.Builder
	appendDefines(&b, iconFS, "icons")
	appendDefines(&b, illustrationFS, "illustrations")
	return b.String()
})

func appendDefines(b *strings.Builder, fsys embed.FS, dir string) {
	entries, err := fsys.ReadDir(dir)
	if err != nil {
		panic(fmt.Sprintf("templates: reading embedded %s dir: %v", dir, err))
	}
	for _, e := range entries {
		if e.IsDir() {
			continue
		}
		name := strings.TrimSuffix(e.Name(), ".html.tmpl")
		body, err := fsys.ReadFile(dir + "/" + e.Name())
		if err != nil {
			panic(fmt.Sprintf("templates: reading embedded %s %q: %v", dir, e.Name(), err))
		}
		fmt.Fprintf(b, `{{- define "%s/%s" -}}%s{{- end -}}`+"\n", dir, name, body)
	}
}
