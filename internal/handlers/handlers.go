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
	"github.com/tylermmorton/tmpl"
)

var validate = validator.New(validator.WithRequiredStructEnabled())

func Index(env string) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		sData := sessions.GetUser(r.Context())
		page := templates.PageIndex{
			IsAuthed: sData != nil,
			Intro:    "Stelo keeps BitCraft assets in digital accounts so you can send, receive, and build with them whether you're online or not. Each asset has its own balance; transfers move value between accounts instantly.",
			Steps: []templates.PageIndexStep{{
				Number: "01",
				Title:  "Get assets",
				Body:   "Bring value onto Stelo through that asset's issuer, or receive a transfer from someone who already holds it.",
			}, {
				Number: "02",
				Title:  "Send them",
				Body:   "Move balances between accounts instantly. To friends, for settlements, or via payment links. No in-game proximity required.",
			}, {
				Number: "03",
				Title:  "Cash out",
				Body:   "For redeemable assets, return them to the issuer and take the items back into BitCraft.",
			}},
			Assets: []templates.PageIndexAsset{{
				Name:        "Hexcoin",
				Issuer:      "Stelo Bank",
				Description: "BitCraft's hexcoin on Stelo. Deposit and redeem 1:1 through the official bank; transfer freely between accounts.",
				TypeLabel:   "In-game item",
			}},
		}

		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		err := templates.Index.Render(w, templates.PublicLayout(page, env))
		if err != nil {
			panic(err)
		}
	})
}

func Login(env string, sessionsKV jetstream.KeyValue) http.HandlerFunc {
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

		redirect := r.URL.Query().Get("redirect")
		if redirect != "" && isValidRedirectURL(redirect) {
			c := &http.Cookie{
				Name:     "auth_redirect",
				Value:    redirect,
				Path:     "/",
				MaxAge:   180,
				HttpOnly: true,
				Secure:   true,
				SameSite: http.SameSiteLaxMode,
			}
			http.SetCookie(w, c)
		} else if !loggingIn && !r.URL.Query().Has("redirect") {
			c := &http.Cookie{
				Name:     "auth_redirect",
				Value:    "",
				Path:     "/",
				MaxAge:   -1,
				HttpOnly: true,
				Secure:   true,
				SameSite: http.SameSiteLaxMode,
			}
			http.SetCookie(w, c)
		}

		if loggingIn {
			publicCode := uniuri.NewLen(12)
			sse := datastar.NewSSE(w, r)

			buff := new(bytes.Buffer)
			err := templates.Login.Render(buff, templates.PublicLayout(templates.PageLogin{
				Code: publicCode,
			}, env), tmpl.WithTarget("page-content"))
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
		loop:
			for time.Now().Before(start.Add(time.Minute)) {
				select {
				case <-r.Context().Done():
					break loop
				case <-time.After(time.Second):
				}

				req, err := http.NewRequestWithContext(r.Context(), http.MethodPost, "https://bitjita.com/api/auth/chat/validate", bytes.NewBuffer(data))
				if err != nil {
					// TODO: Log or something
					continue
				}
				req.Header.Add("User-Agent", "SteloFinance/0.4.0")
				req.Header.Add("Content-Type", "application/json")

				resp, err := client.Do(req)
				if err != nil {
					// TODO: Log or something
					continue
				}
				var data ValidateResponse
				decodeErr := json.NewDecoder(resp.Body).Decode(&data)
				resp.Body.Close()
				if decodeErr != nil {
					// TODO: Log or something
					continue
				}

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
			w.Header().Set("Content-Type", "text/html; charset=utf-8")
			err := templates.Login.Render(w, templates.PublicLayout(templates.PageLogin{}, env))
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
