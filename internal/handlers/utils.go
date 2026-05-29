package handlers

import (
	"strings"
)

func isValidRedirectURL(rawurl string) bool {
	if !strings.HasPrefix(rawurl, "/") {
		return false
	}
	if strings.Contains(rawurl, "//") {
		return false
	}
	if strings.Contains(rawurl, ":") {
		return false
	}

	return true
}
