package accounts

import (
	"errors"
	"fmt"
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

type CreateAccountInput struct {
	UserId int64 // Owner (initial admin of the account)

	Address string
	Code    AccountCode
	Webhook string
}

// func CreateAccount(ctx context.Context, q *gensql.Queries, input CreateAccountInput) (int64, error) {
// 	// Validate the address
// 	if len(input.Address) > MaxAddressLength {
// 		return 0, ErrAddressExceedsLength
// 	}
// 	input.Address = strings.ToUpper(input.Address)
// 	if strings.ContainsFunc(input.Address, func(r rune) bool {
// 		return r < 'A' || r > 'Z'
// 	}) {
// 		return 0, ErrInvalidAccountConfiguration
// 	}

// 	// Prepare account params
// 	accountParams := gensql.InsertAccountParams{
// 		Address:   "",
// 		Webhook:   new(string),
// 		UserID:    new(int64),
// 		LedgerID:  0,
// 		Code:      0,
// 		Flags:     0,
// 		CreatedAt: time.Time{},
// 	}

// 	switch input.Code {
// 	case PersonalAcc:
// 		// Ensure user doesn't already have personal account
// 		user, err := q.GetUserByIdForUpdate(ctx, input.UserId)
// 		if err != nil {
// 			return 0, err
// 		}
// 		if user.WalletID != nil {
// 			return 0, errors.New("accounts: already have primary account")
// 		}
// 	case WarehouseAcc:
// 		if input.CollateralPercentage < 0 || input.CollateralPercentage > 9999 {
// 			return 0, errors.New("accounts: invalid collateralPercentage")
// 		}

// 		walletParams.StGeomfromewkb = &input.Location
// 		walletParams.CollateralPercentage = pgtype.Numeric{
// 			Int:   big.NewInt(input.CollateralPercentage),
// 			Exp:   -3,
// 			Valid: true,
// 		}
// 	case DAL, GA:
// 	default:
// 		return 0, errors.New("accounts: unknown AccountCode")
// 	}

// 	walletId, err := q.InsertWallet(ctx, walletParams)
// 	if err != nil {
// 		// TODO: Check if duplicate address
// 		return 0, err
// 	}
// 	_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
// 		WalletID:    walletId,
// 		UserID:      input.UserId,
// 		Permissions: int64(PermAdmin),
// 		UpdatedAt:   time.Now(),
// 		CreatedAt:   time.Now(),
// 	})
// 	if err != nil {
// 		return 0, err
// 	}

// 	if input.Code == PersonalAcc {
// 		// Update users personal wallet id
// 		err := q.UpdateUserWalletId(ctx, gensql.UpdateUserWalletIdParams{
// 			WalletID: &walletId,
// 			UserID:   input.UserId,
// 		})
// 		if err != nil {
// 			return 0, err
// 		}
// 	}

// 	return walletId, nil
// }

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
