package assets

import (
	"crypto/sha256"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"

	"github.com/go-chi/chi/v5"
)

// Web facing prefix on assets in static folder
const AssetPrefix string = "/assets/"

func HttpHandler(r chi.Router) {
	staticHandler := http.FileServer(http.Dir("web/static"))

	r.Group(func(r chi.Router) {
		r.Use(permCache) // Perma cache all static assets, should use cache busting version
		r.Use(versionedAssets)
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

// versionedAssets is Middleware that strips the version from an asset.
// Example: styles.80b2c87c0b9a5af9.css forwards as styles.css
func versionedAssets(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sections := strings.Split(r.URL.Path, ".")
		if len(sections) != 3 {
			next.ServeHTTP(w, r)
			return
		}

		r.URL.Path = strings.Join([]string{sections[0], sections[2]}, ".")
		next.ServeHTTP(w, r)
	})
}

// GetHashedAssetPath takes the web facing path of an asset, and returns a hashed path to the asset
func GetHashedAssetPath(webPath string) string {
	trimmedPath := strings.TrimPrefix(webPath, AssetPrefix)
	ext := filepath.Ext(webPath)
	if ext == "" {
		panic("no extension found")
	}

	data, err := os.ReadFile("web/static/" + trimmedPath)
	if err != nil {
		return fmt.Sprintf("%v.x%v", strings.TrimSuffix(trimmedPath, ext), ext)
	}

	// TODO: Somehow cache these hashes
	return fmt.Sprintf(AssetPrefix+"%v.%x%v", strings.TrimSuffix(trimmedPath, ext), sha256.Sum256(data), ext)
}
