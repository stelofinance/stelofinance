package accounts

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"strings"
	"time"

	"github.com/dchest/uniuri"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type AccountCode int32

const (
	// 0-99 Liability (credit) type accounts

	// Stelo Ran Account (officially run account)
	SRA AccountCode = 0

	// Player Ran Account
	PRA AccountCode = 1

	// 100-199 Debit accounts, generally just user accounts

	GA AccountCode = 100
	// TODO: Is this account type needed anymore?
	// PersonalAcc AccountCode = 101
)

func (a AccountCode) IsCredit() bool {
	code := int32(a)
	switch {
	case code >= 0 && code <= 99:
		return true
	case code >= 100 && code <= 199:
		return false
	default:
		return false
	}
}

func (a AccountCode) IsDebit() bool {
	return !a.IsCredit()
}

func (a AccountCode) IsValid() bool {
	switch a {
	case GA, PRA, SRA:
		return true
	default:
		return false
	}
}

// IdentifyTrCode will identify which TrCode the transaction is based on the
// sending and receiving AccountCode.
func (sendingAcc AccountCode) IdentifyTrCode(receivingAcc AccountCode) TrCode {
	if sendingAcc.IsDebit() && receivingAcc.IsDebit() {
		return TrAsset
	} else if sendingAcc.IsCredit() && receivingAcc.IsCredit() {
		return TrLiability
	} else if sendingAcc.IsDebit() && receivingAcc.IsCredit() {
		return TrRedeem
	} else if sendingAcc.IsCredit() && receivingAcc.IsDebit() {
		return TrIssue
	} else {
		// Shouldn't ever be hit...
		return TrCode(-1)
	}
}

var ErrInvalidAccountConfiguration = errors.New("accounts: invalid account configuration")
var ErrAddressExceedsLength = fmt.Errorf("accounts: address exceeds max length (%v)", MaxAddressLength)
var ErrDuplicateAddress = fmt.Errorf("accounts: address already taken")

const MaxAddressLength int = 16

// Easy to read and not mistake letters (20 in total)
var AddressStdChars = []byte("ABCDEFGHJKMNPRTUVWXY")

type CreateAccountInput struct {
	OwnerId   int64 // Owner (initial admin of the account)
	isPrimary bool  // Should the account be the primary for the OwnerId user

	Address  string
	Webhook  *string
	LedgerId int64
	Code     AccountCode
}

// CreateAccount should always be called with a transaction that has foreign keys PRAGMA enabled.
func CreateAccount(ctx context.Context, q *gensql.Queries, input CreateAccountInput) (int64, error) {
	// Validate the address
	if len(input.Address) == 0 {
		input.Address = uniuri.NewLenChars(8, AddressStdChars)
		// TODO: double check that address isn't taken...
	} else if len(input.Address) > MaxAddressLength {
		return 0, ErrAddressExceedsLength
	}
	input.Address = strings.ToUpper(input.Address)
	if strings.ContainsFunc(input.Address, func(r rune) bool {
		return r < 'A' || r > 'Z'
	}) {
		return 0, ErrInvalidAccountConfiguration
	}

	// Verify webhook
	if input.Webhook != nil {
		_, err := url.ParseRequestURI(*input.Webhook)
		if err != nil {
			return 0, err
		}
	}

	// Validate account code
	if !input.Code.IsValid() {
		return 0, ErrInvalidAccountConfiguration
	}

	var user *int64
	if input.isPrimary {
		user = &input.OwnerId
	}

	// Insert the account and account permissions
	accId, err := q.InsertAccount(ctx, gensql.InsertAccountParams{
		Address:   input.Address,
		Webhook:   input.Webhook,
		UserID:    user,
		LedgerID:  input.LedgerId,
		Code:      int64(input.Code),
		Flags:     0,
		CreatedAt: time.Now(),
	})
	if err != nil {
		return 0, err
	}
	q.InsertAccountPerm(ctx, gensql.InsertAccountPermParams{
		AccountID:   accId,
		UserID:      input.OwnerId,
		Permissions: int64(PermAdmin),
		UpdatedAt:   time.Now(),
		CreatedAt:   time.Now(),
	})

	return accId, nil
}

// // Easy to read and not mistake letters (20 in total)
// var AddressStdChars = []byte("ABCDEFGHJKMNPRTUVWXY")

// type CreatePersonalWalletInput struct {
// 	UserId int64
// 	// Address string
// }

// func CreatePersonalWallet(ctx context.Context, q *gensql.Queries, input CreatePersonalWalletInput) (int64, error) {
// 	// 512 billion possible variants
// 	addr := uniuri.NewLenChars(9, AddressStdChars)

// 	// Ensure addr isn't taken, if it is, keep rerolling till you get
// 	// one that isn't
// 	// TODO: Implement this ^

// 	accId, err := CreateAccount(ctx, q, CreateAccountInput{
// 		UserId:  input.UserId,
// 		Address: addr,
// 		Code:    PersonalAcc,
// 	})
// 	if err != nil {
// 		return 0, err
// 	}
// 	return accId, err
// }

// func CreateGeneralWallet(ctx context.Context, q *gensql.Queries, userId int64) (int64, string, error) {
// 	// 512 billion possible variants
// 	addr := uniuri.NewLenChars(9, AddressStdChars)

// 	// Ensure addr isn't taken, if it is, keep rerolling till you get
// 	// one that isn't
// 	// TODO: Implement this ^

// 	accId, err := CreateAccount(ctx, q, CreateAccountInput{
// 		UserId:  userId,
// 		Address: addr,
// 		Code:    GA,
// 	})
// 	if err != nil {
// 		return 0, "", err
// 	}
// 	return accId, addr, err
// }

// func CreateDALWallet(ctx context.Context, q *gensql.Queries, adminUserId int64) (int64, error) {
// 	// 512 billion possible variants
// 	addr := uniuri.NewLenChars(9, AddressStdChars)

// 	// Ensure addr isn't taken, if it is, keep rerolling till you get
// 	// one that isn't
// 	// TODO: Implement this ^

// 	accId, err := CreateAccount(ctx, q, CreateAccountInput{
// 		UserId:  adminUserId,
// 		Address: addr,
// 		Code:    DAL,
// 	})
// 	if err != nil {
// 		return 0, err
// 	}
// 	return accId, err

// }
