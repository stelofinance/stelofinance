package handlers

import (
	"net/http"

	"github.com/stelofinance/stelofinance/database"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

func Warehouses(tmpls *templates.Tmpls, db *database.Database) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		uData := sessions.GetUser(r.Context())
		// wData := sessions.GetWallet(r.Context())

		user, err := db.Q.GetUserById(r.Context(), uData.Id)
		if err != nil {
			w.WriteHeader(http.StatusInternalServerError)
			return
		}
		pfp := ""
		if user.DiscordPfp != nil {
			pfp = *user.DiscordPfp
		}

		tmplData := templates.DataLayoutApp{
			Title:       "Warehouses",
			Description: "All of your warehouses",
			NavData: templates.DataComponentAppNav{
				ForWarehouse: true,
				ProfileImage: pfp,
				Username:     user.DiscordUsername,
			},
			MenuData: templates.DataComponentAppMenu{
				ForWarehouse: true,
				ActivePage:   "warehouses",
			},
			// PageData:    nil,
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err = tmpls.ExecuteTemplate(w, "pages/warehouses", tmplData)
		if err != nil {
			panic(err)
		}
	}
}
