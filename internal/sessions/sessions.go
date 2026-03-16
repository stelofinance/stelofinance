package sessions

import (
	"context"
)

type userCtxKey struct{}

var userContextKey = userCtxKey{}

type UserData struct {
	Id         int64  `json:"userId"` // The user's id
	BitcraftId string `json:"bitcraftId"`
	// DiscordId string `json:"discordId"`
}

func WithUser(ctx context.Context, data *UserData) context.Context {
	return context.WithValue(ctx, userContextKey, data)
}

// GetUser will return the user data in the Context.
// If the user data isn't found, nil is returned.
func GetUser(ctx context.Context) *UserData {
	val := ctx.Value(userContextKey)
	if val == nil {
		return nil
	}

	data, ok := ctx.Value(userContextKey).(*UserData)
	if !ok {
		panic("sessions: user context value of wrong type")
	}
	return data
}

type accountCtxKey struct{}

var accountContextKey = accountCtxKey{}

type AccountData struct {
	Id int64 // The account's id
}

func WithAccount(ctx context.Context, data *AccountData) context.Context {
	return context.WithValue(ctx, accountContextKey, data)
}

// GetAccount will return the account data in the Context.
// If the account data isn't found, nil is returned.
func GetAccount(ctx context.Context) *AccountData {
	val := ctx.Value(accountContextKey)
	if val == nil {
		return nil
	}

	data, ok := val.(*AccountData)
	if !ok {
		panic("sessions: wallet context value of wrong type")
	}
	return data
}
