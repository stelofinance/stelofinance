package logger

import (
	"encoding/json"
	"strconv"

	"github.com/nats-io/nats.go"
)

type Log struct {
	Message string         `json:"message"`
	Data    map[string]any `json:"data"`
	Level   Level          `json:"level"`
}

type Level uint8

const (
	FatalLevel Level = iota
	ErrorLevel
	WarnLevel
	InfoLevel
	DebugLevel
	TraceLevel
)

type Logger struct {
	level Level
	nc    *nats.Conn
}

func NewLogger(defaultLvl Level, nc *nats.Conn) *Logger {
	lgr := &Logger{
		level: defaultLvl,
		nc:    nc,
	}

	go func() {
		_, err := nc.Subscribe("logs.level", func(msg *nats.Msg) {
			levelStr := string(msg.Data)
			lvlNum, err := strconv.ParseUint(levelStr, 10, 8)
			if err != nil {
				lgr.Log(Log{
					Message: "unable to set log level",
					Data: map[string]any{
						"error": err.Error(),
					},
					Level: ErrorLevel,
				})
				return
			}
			lgr.level = Level(lvlNum)
		})
		if err != nil {
			lgr.Log(Log{
				Message: "unable to subscribe to log.level",
				Data: map[string]any{
					"error": err.Error(),
				},
				Level: ErrorLevel,
			})
		}
	}()

	return lgr
}

func (l *Logger) Log(log Log) error {
	if log.Level > l.level {
		return nil
	}
	data, err := json.Marshal(log)
	if err != nil {
		return err
	}
	return l.nc.Publish("logs", data)
}
