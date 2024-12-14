package helpers

import (
	"bytes"
	"html/template"
	"os"

	. "maragu.dev/gomponents"
)

// RenderStaticToRaw will return an empty string when the file isn't found
func RenderStaticToRaw(filePath string) Node {
	stuff, err := os.ReadFile("web/static/" + filePath)
	if err != nil {
		return Raw("")
	}

	return Raw(string(stuff))
}

// RenderSVG will return an empty string when the file is not found,
// to avoid returning an error
func RenderSVG(templatePath, classes string) Node {
	tmpl, err := template.ParseFiles("web/templates/" + templatePath)
	if err != nil {
		return Raw("")
	}
	var buf bytes.Buffer
	tmpl.Execute(&buf, classes)

	return Raw(buf.String())
}

// RenderLibHTML will return an empty string when the file isn't found
func RenderLibHTML(templatePath string) Node {
	tmpl, err := template.ParseFiles("web/libs/" + templatePath)
	if err != nil {
		return Raw("")
	}
	var buf bytes.Buffer
	tmpl.Execute(&buf, nil)

	return Raw(buf.String())
}
