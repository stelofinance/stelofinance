package templates

import (
	"errors"
	"html/template"
	"io"
	"io/fs"
	"os"
	"strings"

	"github.com/bmatcuk/doublestar/v4"
	"github.com/stelofinance/stelofinance/internal/assets"
)

var ErrTemplateNotFound = errors.New("templates: template not found")

type Tmpls struct {
	commonTmpls  *template.Template            // Everything except pages so far
	variantTmpls map[string]*template.Template // So far only pages
}

// Rules to this system though are:
// 1. There are common templates and variant templates, common templates DONT
//    redefine any {{block}} anywhere, whereas variant templates do, such as pages.
// 2. Variant templates must not redefine a {{block}} that is in another variant
//    template. (I think this limitation can be overcome though)

// LoadTemplates will load all templates from the passed fsys that have the
// extension specified. The returned templates' names will be the full filepath
// minus any specified prefix and the extension will be trimmed off.
//
// Currently LoadTemplates also creates separate template trees for anything
// in a parent "pages" directory, where the parent directory is selected after
// trimming the prefix.
func LoadTemplates(fsys fs.FS, prefix, extension string) (*Tmpls, error) {
	tmpl := template.New("#blankforfuncs#")
	tmpl.Funcs(addCustomFuncs())

	variantTmpls := make(map[string]*template.Template)

	err := doublestar.GlobWalk(fsys, "*/**/*"+extension, func(fullPath string, d fs.DirEntry) error {
		if d.IsDir() {
			return fs.SkipDir
		}

		b, err := fs.ReadFile(fsys, fullPath)
		if err != nil {
			return err
		}
		trimmedName := strings.TrimSuffix(strings.TrimPrefix(fullPath, prefix), extension)

		if strings.HasPrefix(trimmedName, "pages") {
			tmpl, err := tmpl.Clone()
			if err != nil {
				return err
			}
			tmpl, err = tmpl.New(trimmedName).Parse(string(b))
			if err != nil {
				return err
			}
			variantTmpls[trimmedName] = tmpl
		} else {
			tmpl, err = tmpl.New(trimmedName).Parse(string(b))
			if err != nil {
				return err
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}

	return &Tmpls{
		commonTmpls:  tmpl,
		variantTmpls: variantTmpls,
	}, nil
}

func (t *Tmpls) ExecuteTemplate(wr io.Writer, name string, data any) error {
	tmpl := t.commonTmpls.Lookup(name)
	if tmpl != nil {
		return tmpl.Execute(wr, data)
	}

	tmpl, ok := t.variantTmpls[name]
	if !ok {
		return ErrTemplateNotFound
	}
	return tmpl.Execute(wr, data)
}

func addCustomFuncs() template.FuncMap {
	funcs := make(template.FuncMap)

	funcs["hash_asset_path"] = assets.GetHashedAssetPath
	funcs["raw_asset_string"] = assetToRawString

	return funcs
}

func assetToRawString(safeType, file string) any {
	fileBytes, err := os.ReadFile("web/static/" + file)
	if err != nil {
		return ""
	}
	rawStr := string(fileBytes)

	if safeType == "CSS" {
		return template.CSS(rawStr)
	}

	return rawStr
}
