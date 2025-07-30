package accounts

type Permission uint64

const PermNone Permission = 0

const (
	// Wallet Management
	// these permissions are for wallet managed related permissions

	// Complete control of the wallet
	PermAdmin Permission = 1 << iota
	PermRESERVED2
	PermRESERVED3
	PermRESERVED4
	PermRESERVED5
	PermRESERVED6
	PermRESERVED7
	PermRESERVED8
	PermRESERVED9
	PermRESERVED10
	PermRESERVED11
	PermRESERVED12
	PermRESERVED13
	PermRESERVED14
	PermRESERVED15
	PermRESERVED16

	// Wallet Actions
	// these permissions are for specific actions on the wallet

	PermReadBals // Read account balances
	// PermRESERVED2
	// PermRESERVED3
	// PermRESERVED4
	// PermRESERVED5
	// PermRESERVED6
	// PermRESERVED7
	// PermRESERVED8
	// PermRESERVED9
	// PermRESERVED10
	// PermRESERVED11
	// PermRESERVED12
	// PermRESERVED13
	// PermRESERVED14
	// PermRESERVED15
	// PermRESERVED16
)
