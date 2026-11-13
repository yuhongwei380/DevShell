package app

import (
	"encoding/base64"
	"encoding/json"
	"errors"
)

// Native terminal mouse reports may contain non-UTF-8 bytes. Keep the existing
// browser string protocol while allowing GPUI to preserve these bytes exactly.
func (m *relayMessage) UnmarshalJSON(data []byte) error {
	type plain relayMessage
	var input struct {
		plain
		DataBase64 *string `json:"dataBase64"`
	}
	if err := json.Unmarshal(data, &input); err != nil {
		return err
	}
	*m = relayMessage(input.plain)
	if input.DataBase64 != nil {
		if len(*input.DataBase64) > base64.StdEncoding.EncodedLen(65536) {
			return errors.New("终端输入过大")
		}
		bytes, err := base64.StdEncoding.DecodeString(*input.DataBase64)
		if err != nil {
			return errors.New("终端输入编码无效")
		}
		if len(bytes) > 65536 {
			return errors.New("终端输入过大")
		}
		m.Data = string(bytes)
	}
	return nil
}
