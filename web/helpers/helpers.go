package helpers

import (
	"bytes"
	"html/template"

	. "maragu.dev/gomponents"
)

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
