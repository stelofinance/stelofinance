package handlers

import (
	"bytes"
	"encoding/json"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/dchest/uniuri"
	"github.com/go-playground/validator/v10"
	"github.com/nats-io/nats.go/jetstream"
	"github.com/starfederation/datastar-go/datastar"
	"github.com/stelofinance/stelofinance/internal/sessions"
	"github.com/stelofinance/stelofinance/web/templates"
)

var validate = validator.New(validator.WithRequiredStructEnabled())

func Index(tmpls *templates.Tmpls) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetUser(r.Context())
		tmplData := templates.LayoutPrimary{
			NavData: templates.ComponentNav{},
			FooterData: templates.ComponentFooter{
				Links: []templates.ComponentFooterLink{{
					Href: "https://discord.gg/t6gM7v7V7T",
					Text: "Discord",
				}, {
					Href: "https://github.com/stelofinance/stelofinance/tree/main/docs",
					Text: "Docs",
				}, {
					Href: "https://github.com/stelofinance",
					Text: "GitHub",
				}},
			},
			PageData: templates.PageIndex{
				IsAuthed: sData != nil,
				InfoCards: []templates.PageIndexInfoCard{{
					Title: "From Physical to Digial",
					Body:  "Assets on Stelo range from purely digital to 1:1 backed with real redeemable items in-game (and in-between)!",
				}, {
					Title: "Built to be Built Upon",
					Body:  "The Stelo platform enables you to create whatever financial product you can dream of, from loan services to nation state bonds.",
				}, {
					Title: "Permissioned Control",
					Body:  "Stelo gives you a range of granular permissions, so you can be confident in managing and delegating your finances.",
				}, {
					Title: "An Open Platform",
					Body:  "Stelo provides a public API so anyone can leverage the platform to build their idea. Need more functionality? Join the Discord and ask!",
				}, {
					Title: "Global Exchange",
					Body:  "What good are all these assets if they can't be traded? That's why Stelo has a built in order-book global market, to maximize the liquidity of all your assets.",
				}, {
					Title: "Instantaneous",
					Body:  "Trade at the speed of light. Whether you're in different towns, regions, or not even online, you can trade all your assets instantly with anyone.",
				}},
			},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := tmpls.ExecuteTemplate(w, "pages/index", tmplData)
		if err != nil {
			panic(err)
		}
	})
}

func Login(tmpls *templates.Tmpls, sessionsKV jetstream.KeyValue) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		loggingIn := false
		if r.URL.Query().Has("datastar") {
			type input struct {
				LoggingIn bool `json:"loggingIn"`
			}
			var ds input
			err := json.Unmarshal([]byte(r.URL.Query().Get("datastar")), &ds)
			if err != nil {
				w.WriteHeader(http.StatusBadRequest)
				return
			}
			loggingIn = ds.LoggingIn
		}

		if loggingIn {
			publicCode := uniuri.NewLen(12)
			tmplData := templates.DefaultLayoutPrimary
			tmplData.PageData = templates.PageLogin{
				OnlyRenderPage: true,
				Code:           publicCode,
			}
			sse := datastar.NewSSE(w, r)

			buff := new(bytes.Buffer)
			err := tmpls.ExecuteTemplate(buff, "pages/login", tmplData)
			if err != nil {
				panic(err)
			}

			sse.PatchElements(buff.String())

			// Now, loop till they auth, timeout after 60 seconds
			start := time.Now()
			client := &http.Client{}
			body := make(map[string]string)
			body["code"] = "stelo:" + publicCode
			data, err := json.Marshal(body)
			if err != nil {
				w.WriteHeader(http.StatusInternalServerError)
				return
			}
			type ValidateResponsePlayer struct {
				EntityId string
				Username string
			}
			type ValidateResponse struct {
				Success bool                   `json:"success"`
				Player  ValidateResponsePlayer `json:"player"`
			}
			for time.Now().Before(start.Add(time.Minute)) {
				time.Sleep(time.Second)
				req, err := http.NewRequestWithContext(r.Context(), http.MethodPost, "https://bitjita.com/api/auth/chat/validate", bytes.NewBuffer(data))
				if err != nil {
					// TODO: Log or something
					continue
				}
				req.Header.Add("User-Agent", "SteloFinance/0.4.0")

				resp, err := client.Do(req)
				if err != nil {
					// TODO: Log or something
					continue
				}
				var data ValidateResponse
				if err := json.NewDecoder(resp.Body).Decode(&data); err != nil {
					// TODO: Log or something
					continue
				}
				resp.Body.Close()

				if !data.Success {
					continue
				}

				// Create login kv
				bytes, err := json.Marshal(LoginKV{
					Username: data.Player.Username,
					PlayerId: data.Player.EntityId,
				})
				if err != nil {
					// TODO: add log
					continue
				}
				secretKey := uniuri.New()
				_, err = sessionsKV.Create(r.Context(), "logins."+secretKey, bytes, jetstream.KeyTTL(time.Second*15))
				if err != nil {
					// TODO: add log
					continue
				}

				// Redirect to secret auth endpoint
				sse.Redirect("/auth/" + secretKey)
				return
			}

			// TODO: Handle timeout better
			return
		} else {
			tmplData := templates.DefaultLayoutPrimary
			tmplData.PageData = templates.PageLogin{}

			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			err := tmpls.ExecuteTemplate(w, "pages/login", tmplData)
			if err != nil {
				panic(err)
			}
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
