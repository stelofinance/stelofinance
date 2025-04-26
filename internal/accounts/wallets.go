package accounts

import (
	"context"
	"errors"
	"time"

	"github.com/dchest/uniuri"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type AccountCode int32

const (
	// 0-99 System accounts

	// liability account
	DigitalAssetLiabilityAccountCode AccountCode = 0

	// 100-199 User accounts
	// asset (debit accounts)

	GeneralAccountCode  AccountCode = 100
	PersonalAccountCode AccountCode = 101

	// 200-299 Warehousing related accounts
	// liability (credit accounts)

	WarehouseAccountCode AccountCode = 200
)

func (a AccountCode) IsCredit() bool {
	// TODO: Maybe don't use switch statement
	switch a {
	case DigitalAssetLiabilityAccountCode:
		return true
	case GeneralAccountCode:
		return false
	case PersonalAccountCode:
		return false
	case WarehouseAccountCode:
		return true
	default:
		return false
	}
}

// func CreatePersonalWallet(address string)

var ErrInvalidAccountConfiguration = errors.New("accounts: invalid account configuration")

type createWalletInput struct {
	userId int64 // Admin of the account

	address                   string
	code                      AccountCode
	webhook                   string
	location                  [2]int
	collateralAccountId       int
	collateralLockedAccountId int
	collateralPercentage      float64
}

func createWallet(ctx context.Context, q *gensql.Queries, input createWalletInput) (int64, error) {
	switch input.code {
	case DigitalAssetLiabilityAccountCode:
		// Create wallet
		accGrId, err := q.InsertWallet(ctx, gensql.InsertWalletParams{
			Address:   input.address,
			Code:      int32(DigitalAssetLiabilityAccountCode),
			CreatedAt: time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Create wallet permissions
		_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
			WalletID:    accGrId,
			UserID:      input.userId,
			Permissions: int64(PermIsAdmin),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})
		if err != nil {
			return 0, err
		}
		return accGrId, nil
	case GeneralAccountCode:

	case PersonalAccountCode:
		// Ensure user doesn't already have personal account
		user, err := q.GetUserById(ctx, input.userId)
		if err != nil {
			return 0, err
		}
		if user.WalletID != nil {
			return 0, errors.New("accounts: already have primary account")
		}

		// Create wallet
		accGrId, err := q.InsertWallet(ctx, gensql.InsertWalletParams{
			Address:   input.address,
			Code:      int32(PersonalAccountCode),
			CreatedAt: time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Create wallet permissions
		_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
			WalletID:    accGrId,
			UserID:      input.userId,
			Permissions: int64(PermIsAdmin),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Update users personal wallet id
		err = q.UpdateUserWalletId(ctx, gensql.UpdateUserWalletIdParams{
			WalletID: &accGrId,
			UserID:   input.userId,
		})
		if err != nil {
			return 0, err
		}

		return accGrId, nil
	case WarehouseAccountCode:

	default:
		return 0, errors.New("account: unknown AccountCode")
	}

	return 0, nil
}

// Easy to read and not mistake letters
var AddressStdChars = []byte("ABCDEFGHJKMNPRTUVWXY")

type CreatePersonalWalletInput struct {
	UserId int64
	// Address string
}

func CreatePersonalWallet(ctx context.Context, q *gensql.Queries, input CreatePersonalWalletInput) (int64, error) {
	// 512 billion possible variants
	addr := uniuri.NewLenChars(9, AddressStdChars)

	// Ensure addr isn't taken, if it is, keep rerolling till you get
	// one that isn't
	// TODO: Implement this ^

	accId, err := createWallet(ctx, q, createWalletInput{
		userId:  input.UserId,
		address: addr,
		code:    PersonalAccountCode,
	})
	if err != nil {
		return 0, err
	}
	return accId, err
}

func CreateDigitalAssetLiabilityWallet(ctx context.Context, q *gensql.Queries, adminUserId int64) (int64, error) {
	// 512 billion possible variants
	addr := uniuri.NewLenChars(9, AddressStdChars)

	// Ensure addr isn't taken, if it is, keep rerolling till you get
	// one that isn't
	// TODO: Implement this ^

	accId, err := createWallet(ctx, q, createWalletInput{
		userId:  adminUserId,
		address: addr,
		code:    DigitalAssetLiabilityAccountCode,
	})
	if err != nil {
		return 0, err
	}
	return accId, err

}
