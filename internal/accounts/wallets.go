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

	// Digital Asset Liability account (credit)
	DAL AccountCode = 0

	// 100-199 User accounts
	// asset (debit accounts)

	GeneralAcc  AccountCode = 100
	PersonalAcc AccountCode = 101

	// 200-299 Warehousing related accounts

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

// IdentifyTxCode will identify which TxCode the transaction is based on the
// sending and receiving AccountCode. NOTE, if TxWarehouseTransfer is identified
// you should check if it is actually a TxCollateral.
func (sendingAcc AccountCode) IdentifyTxCode(receivingAcc AccountCode) TxCode {
	// Define valid pairs for each TxCode
	type txPair struct {
		txCode TxCode
		pairs  [][2]AccountCode
	}

	txPairs := []txPair{
		{
			txCode: TxSysUser,
			pairs: [][2]AccountCode{
				{DAL, GeneralAcc},
				{DAL, PersonalAcc},
			},
		},
		{
			txCode: TxUserToUser,
			pairs: [][2]AccountCode{
				{GeneralAcc, GeneralAcc},
				{GeneralAcc, PersonalAcc},
				{PersonalAcc, PersonalAcc},
			},
		},
		{
			txCode: TxWarehouseTransfer,
			pairs: [][2]AccountCode{
				{PersonalAcc, WarehouseAcc},
			},
		},
		{
			txCode: TxWarehouseToWarehouseTransfer,
			pairs: [][2]AccountCode{
				{WarehouseAcc, WarehouseAcc},
			},
		},
		// TxCollateral has no pairs, so it’s omitted
	}

	for _, txc := range txPairs {
		for _, pair := range txc.pairs {
			// Match if (sender, receiver) equals pair or its reverse
			if (sendingAcc == pair[0] && receivingAcc == pair[1]) ||
				(sendingAcc == pair[1] && receivingAcc == pair[0]) {
				return txc.txCode
			}
		}
	}

	return -1
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
var ErrAddressExceedsLength = fmt.Errorf("accounts: address exceeds max length (%v)", MaxAddressLength)
var ErrDuplicateAddress = fmt.Errorf("accounts: address already taken")

const MaxAddressLength int = 16

type CreateWalletInput struct {
	UserId int64 // Admin of the account

	Address  string
	Code     AccountCode
	Webhook  string
	Location postgis.Point

	// collateralAccountId       int   // Probably not needed, will be created
	// collateralLockedAccountId int   // Probably not needed, will be created
	CollateralPercentage int64 // Must be between >= 0 and <= 9999 (scale of 3, so 0 to 9.999)
}

func CreateWallet(ctx context.Context, q *gensql.Queries, input CreateWalletInput) (int64, error) {
	// Validate the address
	if len(input.Address) > MaxAddressLength {
		return 0, ErrAddressExceedsLength
	}
	input.Address = strings.ToUpper(input.Address)
	if strings.ContainsFunc(input.Address, func(r rune) bool {
		return r < 'A' || r > 'Z'
	}) {
		return 0, ErrInvalidAccountConfiguration
	}

	// Prepare wallet params
	walletParams := gensql.InsertWalletParams{
		Address:   input.Address,
		Code:      int32(input.Code),
		CreatedAt: time.Now(),
	}

	switch input.Code {
	case PersonalAcc:
		// Ensure user doesn't already have personal account
		user, err := q.GetUserByIdForUpdate(ctx, input.UserId)
		if err != nil {
			return 0, err
		}
		if user.WalletID != nil {
			return 0, errors.New("accounts: already have primary account")
		}
	case WarehouseAcc:
		if input.CollateralPercentage < 0 || input.CollateralPercentage > 9999 {
			return 0, errors.New("accounts: invalid collateralPercentage")
		}

		walletParams.StGeomfromewkb = &input.Location
		walletParams.CollateralPercentage = pgtype.Numeric{
			Int:   big.NewInt(input.CollateralPercentage),
			Exp:   -3,
			Valid: true,
		}
	case DAL, GeneralAcc:
	default:
		return 0, errors.New("accounts: unknown AccountCode")
	}

	walletId, err := q.InsertWallet(ctx, walletParams)
	if err != nil {
		// TODO: Check if duplicate address
		return 0, err
	}
	_, err = q.InsertWalletPermission(ctx, gensql.InsertWalletPermissionParams{
		WalletID:    walletId,
		UserID:      input.UserId,
		Permissions: int64(PermAdmin),
		UpdatedAt:   time.Now(),
		CreatedAt:   time.Now(),
	})
	if err != nil {
		return 0, err
	}

	if input.Code == PersonalAcc {
		// Update users personal wallet id
		err := q.UpdateUserWalletId(ctx, gensql.UpdateUserWalletIdParams{
			WalletID: &walletId,
			UserID:   input.UserId,
		})
		if err != nil {
			return 0, err
		}
	}

	return walletId, nil
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

	accId, err := CreateWallet(ctx, q, CreateWalletInput{
		UserId:  input.UserId,
		Address: addr,
		Code:    PersonalAcc,
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

	accId, err := CreateWallet(ctx, q, CreateWalletInput{
		UserId:  userId,
		Address: addr,
		Code:    GeneralAcc,
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

	accId, err := CreateWallet(ctx, q, CreateWalletInput{
		UserId:               input.UserId,
		Address:              addr,
		Code:                 WarehouseAcc,
		Location:             input.Location,
		CollateralPercentage: input.CollateralPercentage,
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

	accId, err := CreateWallet(ctx, q, CreateWalletInput{
		UserId:  adminUserId,
		Address: addr,
		Code:    DAL,
	})
	if err != nil {
		return 0, err
	}
	return accId, err

}
