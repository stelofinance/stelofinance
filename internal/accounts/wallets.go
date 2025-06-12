package accounts

import (
	"context"
	"errors"
	"fmt"
	"math/big"
	"strings"
	"time"

	"github.com/cridenour/go-postgis"
	"github.com/dchest/uniuri"
	"github.com/jackc/pgx/v5/pgtype"
	"github.com/stelofinance/stelofinance/database/gensql"
)

type AccountCode int32

const (
	// 0-99 System accounts

	// Digital Asset Liability account
	DAL AccountCode = 0

	// 100-199 User accounts
	// asset (debit accounts)

	GeneralAcc  AccountCode = 100
	PersonalAcc AccountCode = 101

	// 200-299 Warehousing related accounts
	// liability (credit accounts)

	WarehouseAcc          AccountCode = 200 // liability (credit account)
	WarehouseCollatAcc    AccountCode = 201 // asset (debit account)
	WarehouseCollatLkdAcc AccountCode = 202 // asset (debit account)
)

func (a AccountCode) IsCredit() bool {
	// TODO: Maybe don't use switch statement
	switch a {
	case DAL, WarehouseAcc:
		return true
	case GeneralAcc, PersonalAcc:
		return false
	default:
		return false
	}
}

func (a AccountCode) IsDebit() bool {
	return !a.IsCredit()
}

func (a AccountCode) IsUserCode() bool {
	if int32(a) >= 100 && int32(a) <= 199 {
		return true
	}
	return false
}

// func CreatePersonalWallet(address string)

var ErrInvalidAccountConfiguration = errors.New("accounts: invalid account configuration")
var ErrAddressExceedsLength = errors.New(fmt.Sprintf("accounts: address exceeds max length (%v)", MaxAddressLength))

const MaxAddressLength int = 16

type createWalletInput struct {
	userId int64 // Admin of the account

	address  string
	code     AccountCode
	webhook  string
	location postgis.Point

	collateralAccountId       int   // Probably not needed, will be created
	collateralLockedAccountId int   // Probably not needed, will be created
	collateralPercentage      int64 // Must be between >= 0 and <= 9999 (scale of 3, so 0 to 9.999)
}

func createWallet(ctx context.Context, q *gensql.Queries, input createWalletInput) (int64, error) {
	if len(input.address) > MaxAddressLength {
		return 0, ErrAddressExceedsLength
	}
	input.address = strings.ToUpper(input.address)
	if strings.ContainsFunc(input.address, func(r rune) bool {
		return r < 'A' || r > 'Z'
	}) {
		return 0, ErrInvalidAccountConfiguration
	}

	switch input.code {
	case DAL:
		// Create wallet
		walletId, err := q.InsertWallet(ctx, gensql.InsertWalletParams{
			Address:   input.address,
			Code:      int32(DAL),
			CreatedAt: time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Create wallet permissions
		_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
			WalletID:    walletId,
			UserID:      input.userId,
			Permissions: int64(PermAdmin),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})
		if err != nil {
			return 0, err
		}
		return walletId, nil
	case GeneralAcc:
		// Create wallet
		walletId, err := q.InsertWallet(ctx, gensql.InsertWalletParams{
			Address:   input.address,
			Code:      int32(GeneralAcc),
			CreatedAt: time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Create wallet permissions
		_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
			WalletID:    walletId,
			UserID:      input.userId,
			Permissions: int64(PermAdmin),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})
		if err != nil {
			return 0, err
		}

		return walletId, nil
	case PersonalAcc:
		// Ensure user doesn't already have personal account
		user, err := q.GetUserById(ctx, input.userId)
		if err != nil {
			return 0, err
		}
		if user.WalletID != nil {
			return 0, errors.New("accounts: already have primary account")
		}

		// Create wallet
		walletId, err := q.InsertWallet(ctx, gensql.InsertWalletParams{
			Address:   input.address,
			Code:      int32(PersonalAcc),
			CreatedAt: time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Create wallet permissions
		_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
			WalletID:    walletId,
			UserID:      input.userId,
			Permissions: int64(PermAdmin),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})
		if err != nil {
			return 0, err
		}

		// Update users personal wallet id
		err = q.UpdateUserWalletId(ctx, gensql.UpdateUserWalletIdParams{
			WalletID: &walletId,
			UserID:   input.userId,
		})
		if err != nil {
			return 0, err
		}

		return walletId, nil
	case WarehouseAcc:
		if input.collateralPercentage < 0 || input.collateralPercentage > 9999 {
			return 0, errors.New("accounts: invalid collateralPercentage")
		}

		walletId, err := q.InsertWallet(ctx, gensql.InsertWalletParams{
			Address:        input.address,
			Code:           int32(WarehouseAcc),
			CreatedAt:      time.Now(),
			StGeomfromewkb: &input.location,
			CollateralPercentage: pgtype.Numeric{
				Int:   big.NewInt(input.collateralPercentage),
				Exp:   -3,
				Valid: true,
			},
		})
		if err != nil {
			return 0, err
		}

		// Create wallet permissions
		_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
			WalletID:    walletId,
			UserID:      input.userId,
			Permissions: int64(PermAdmin),
			UpdatedAt:   time.Now(),
			CreatedAt:   time.Now(),
		})
		if err != nil {
			return 0, err
		}
		return walletId, nil
	default:
		return 0, errors.New("accounts: unknown AccountCode")
	}
}

// Easy to read and not mistake letters (20 in total)
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
		code:    PersonalAcc,
	})
	if err != nil {
		return 0, err
	}
	return accId, err
}

func CreateGeneralWallet(ctx context.Context, q *gensql.Queries, userId int64) (int64, string, error) {
	// 512 billion possible variants
	addr := uniuri.NewLenChars(9, AddressStdChars)

	// Ensure addr isn't taken, if it is, keep rerolling till you get
	// one that isn't
	// TODO: Implement this ^

	accId, err := createWallet(ctx, q, createWalletInput{
		userId:  userId,
		address: addr,
		code:    GeneralAcc,
	})
	if err != nil {
		return 0, "", err
	}
	return accId, addr, err
}

type CreateWarehouseInput struct {
	UserId               int64
	Addr                 string // optional, if not provided a random is generated
	Location             postgis.Point
	CollateralPercentage int64 // Must be between >= 0 and <= 9999 (scale of 3)
}

func CreateWarehouseWallet(ctx context.Context, q *gensql.Queries, input CreateWarehouseInput) (int64, error) {
	addr := input.Addr
	if addr == "" {
		// 512 billion possible variants
		addr = uniuri.NewLenChars(9, AddressStdChars)

		// Ensure addr isn't taken, if it is, keep rerolling till you get
		// one that isn't
		// TODO: Implement this ^
	}

	accId, err := createWallet(ctx, q, createWalletInput{
		userId:               input.UserId,
		address:              addr,
		code:                 WarehouseAcc,
		location:             input.Location,
		collateralPercentage: input.CollateralPercentage,
	})
	if err != nil {
		return 0, err
	}

	return accId, err
}

func CreateDALWallet(ctx context.Context, q *gensql.Queries, adminUserId int64) (int64, error) {
	// 512 billion possible variants
	addr := uniuri.NewLenChars(9, AddressStdChars)

	// Ensure addr isn't taken, if it is, keep rerolling till you get
	// one that isn't
	// TODO: Implement this ^

	accId, err := createWallet(ctx, q, createWalletInput{
		userId:  adminUserId,
		address: addr,
		code:    DAL,
	})
	if err != nil {
		return 0, err
	}
	return accId, err

}
