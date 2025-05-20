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

func GetSession(ctx context.Context) (*Data, bool) {
	data, ok := ctx.Value(sessionContextKey).(*Data)
	return data, ok
}
