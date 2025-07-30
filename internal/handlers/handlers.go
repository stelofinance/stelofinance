package handlers

import (
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/nats-io/nats.go/jetstream"
	datastar "github.com/starfederation/datastar/sdk/go"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

func Index(tmpls *templates.Tmpls) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetUser(r.Context())
		tmplData := templates.DataLayoutPrimary{
			NavData: templates.DataComponentNav{},
			FooterData: templates.DataComponentFooter{
				Links: []templates.DataComponentFooterLink{{
					Href: "https://discord.gg/t6gM7v7V7T",
					Text: "Discord",
				}, {
					Href: "https://github.com/stelofinance",
					Text: "GitHub",
				}},
			},
			PageData: templates.DataPageHomepage{
				User: sData != nil,
				InfoCards: []templates.DataPageHomepageInfoCard{{
					Title: "Convienent in every way",
					Body:  "One of Stelo's core goals is to be a convienent way for managing all your finances. Once you've created your account the entire platform is at your fingertips.",
				}, {
					Title: "Connecting the physical to the digial",
					Body:  "Every item in the Stelo ecosystem is backed by the real asset in game. Whenever you want any of your digital goods in game, just visit a Stelo partnered warehouse and you'll receive the items from your account.",
				}, {
					Title: "Built to be built upon",
					Body:  "By leveraging Stelo's app platform you can build loan services, trading bots, tax systems, and so much more! If you're daring enough, you could even build another entire finance platform ontop.",
				}, {
					Title: "A simplistic currency",
					Body:  "The Stelo currency is a divisible, limited supply currency built into the Stelo platform. It's main purpose is to be the collateral against assets stored in Stelo partnered warehouses.",
				}, {
					Title: "A free platform",
					Body:  "Stelo's core functionality is completely free! No monthly subscription, no transactions fees on anything. Stelo will be monetized by other means if needed.",
				}, {
					Title: "A global exchange",
					Body:  "To showcase the power of the smart wallet system, Stelo will be creating a global exchange where users can sell goods to anyone, anytime, anywhere. This utility will be only just the start of the Stelo ecosystem.",
				}},
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "pages/homepage", tmplData)
		if err != nil {
			panic(err)
		}
	})
}

func Login(tmpls *templates.Tmpls) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		tmplData := templates.DataLayoutPrimary{
			NavData: templates.DataComponentNav{},
			FooterData: templates.DataComponentFooter{
				Links: []templates.DataComponentFooterLink{{
					Href: "https://discord.gg/t6gM7v7V7T",
					Text: "Discord",
				}, {
					Href: "https://github.com/stelofinance",
					Text: "GitHub",
				}},
			},
			PageData: nil,
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "pages/login", tmplData)
		if err != nil {
			panic(err)
		}
	}
}

var hotReloadOnce sync.Once

func HotReload() http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sse := datastar.NewSSE(w, r)
		hotReloadOnce.Do(func() {
			// Refresh the client page as soon as connection
			// is established. This will occur only once
			// after the server starts.
			sse.ExecuteScript(
				"window.location.reload()",
				datastar.WithExecuteScriptRetryDuration(time.Second),
			)
		})

		// Freeze the event stream until the connection
		// is lost for any reason. This will force the client
		// to attempt to reconnect after the server reboots.
		<-r.Context().Done()
	}
}

func Logout(sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetUser(r.Context())

		cookie, err := r.Cookie("sid")
		if err != nil {
			http.Redirect(w, r, "/", http.StatusFound)
			return
		}
		sid := strings.TrimPrefix(cookie.Value, "stl_")

		// Delete session
		sessionsKV.Delete(r.Context(), "users."+strconv.FormatInt(sData.Id, 10)+".sessions."+sid)

		// Delete cookie
		c := &http.Cookie{
			Name:     "sid",
			Value:    "",
			Path:     "/",
			MaxAge:   -1,
			HttpOnly: true,
			Secure:   true,
			SameSite: http.SameSiteLaxMode,
		}
		http.SetCookie(w, c)

		// Redirect to homepage
		http.Redirect(w, r, "/", http.StatusFound)
	}
}
