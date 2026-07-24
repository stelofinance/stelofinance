package templates

import (
	"html/template"

	"github.com/stelofinance/stelofinance/internal/assets"
	"github.com/stelofinance/stelofinance/web/static"
)

var globalFuncs = template.FuncMap{
	"hash_asset_path":  assets.GetHashedAssetPath,
	"raw_asset_string": assetToRawString,
}

func assetToRawString(safeType, file string) any {
	fileBytes, err := static.StaticFS.ReadFile(file)
	if err != nil {
		return ""
	}
	rawStr := string(fileBytes)

	if safeType == "CSS" {
		return template.CSS(rawStr)
	}

	return rawStr
}
