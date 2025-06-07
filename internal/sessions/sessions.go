package sessions

import "context"

var userContextKey = struct{}{}

type UserData struct {
	Id        int64  `json:"userId"` // The user's id
	DiscordId string `json:"discordId"`
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

	data, ok := val.(*UserData)
	if !ok {
		panic("sessions: session context value of wrong type")
	}
	return data
}

var walletContextKey = struct{}{}

type WalletData struct {
	Id      int64 // The wallet's id
	Address string
}

func WithWallet(ctx context.Context, data *WalletData) context.Context {
	return context.WithValue(ctx, walletContextKey, data)
}

// GetWallet will return the wallet data in the Context.
// If the wallet data isn't found, nil is returned.
func GetWallet(ctx context.Context) *WalletData {
	val := ctx.Value(walletContextKey)
	if val == nil {
		return nil
	}

	data, ok := val.(*WalletData)
	if !ok {
		panic("sessions: session context value of wrong type")
	}
	return data
}
