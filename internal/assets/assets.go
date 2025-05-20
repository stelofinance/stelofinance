package assets

import (
	"crypto/sha256"
	"fmt"
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
	"github.com/stelofinance/stelofinance/web/static"
)

// Web facing prefix on assets in static folder
const AssetPrefix string = "/assets/"

func HttpHandler(r chi.Router) {
	staticHandler := http.FileServerFS(static.StaticFS)

	r.Group(func(r chi.Router) {
		r.Use(permCache) // Perma cache all static assets, should use cache busting version
		r.Use(stripVersionHash)
		r.Get(AssetPrefix+"*", http.StripPrefix(AssetPrefix, staticHandler).ServeHTTP)
	})
}

func permCache(h http.Handler) http.Handler {
	fn := func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Cache-Control", "max-age=31536000")
		h.ServeHTTP(w, r)
	}

	return http.HandlerFunc(fn)
}

// stripVersionHash is Middleware that strips the version from an asset.
// Example: styles.80b2c87c0b9a5af9.css forwards as styles.css
func stripVersionHash(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sections := strings.Split(r.URL.Path, ".")
		if len(sections) < 3 {
			next.ServeHTTP(w, r)
			return
		}

		// Detect if the first section is likely a hash
		if len(sections[1]) != 64 {
			next.ServeHTTP(w, r)
			return
		}

		noHashSections := append([]string{sections[0]}, sections[2:]...)
		r.URL.Path = strings.Join(noHashSections, ".")
		next.ServeHTTP(w, r)
	})
}

// GetHashedAssetPath takes the web facing path of an asset, and returns a hashed path to the asset.
// webPath must contain a file extension, lest the function panic
func GetHashedAssetPath(webPath string) string {
	trimmedPath := strings.TrimPrefix(webPath, AssetPrefix)
	strs := strings.SplitN(webPath, ".", 2)
	if len(strs) < 2 {
		panic("invalid file passed to GetHashedAssetPath")
	}
	ext := "." + strs[len(strs)-1]

	data, err := static.StaticFS.ReadFile(trimmedPath)
	if err != nil {
		return fmt.Sprintf("%v.x%v", strings.TrimSuffix(trimmedPath, ext), ext)
	}

	// TODO: Somehow cache these hashes
	return fmt.Sprintf(AssetPrefix+"%v.%x%v", strings.TrimSuffix(trimmedPath, ext), sha256.Sum256(data), ext)
}
