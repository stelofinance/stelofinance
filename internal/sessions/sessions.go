package sessions

import "context"

var sessionContextKey = struct{}{}

type Data struct {
	UserId    int64  `json:"userId"`
	DiscordId string `json:"discordId"`
}

func WithSession(ctx context.Context, data *Data) context.Context {
	return context.WithValue(ctx, sessionContextKey, data)
}

// GetSession will return the session data in the Context.
// If the session data isn't found, nil is returned.
func GetSession(ctx context.Context) *Data {
	val := ctx.Value(sessionContextKey)
	if val == nil {
		return nil
	}

	data, ok := val.(*Data)
	if !ok {
		panic("sessions: session context value of wrong type")
	}
	return data
}
