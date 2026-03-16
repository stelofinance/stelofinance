package accounts

type Permission uint64

const PermNone Permission = 0

const (
	// Account Management
	// these permissions are for account managed related permissions

	// Complete control of the account
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

	// Account Actions
	// these permissions are for specific actions on the account

	PermReadBal // Read account balance
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
