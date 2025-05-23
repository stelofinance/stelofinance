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

func GetSession(ctx context.Context) (data *Data, found bool) {
	val := ctx.Value(sessionContextKey)
	if val == nil {
		return nil, false
	}

	data, ok := ctx.Value(sessionContextKey).(*Data)
	if !ok {
		panic("sessions: session context value of wrong type")
	}
	return data, true
}
