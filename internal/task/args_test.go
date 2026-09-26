package task

import (
	"flag"
	"reflect"
	"testing"
)

func TestReorderInterspersed(t *testing.T) {
	fs := flag.NewFlagSet("mdfu", flag.ContinueOnError)
	fs.Int("limit", 50, "")
	fs.Bool("hidden", false, "")
	fs.String("root", ".", "")

	tests := []struct {
		name string
		args []string
		want []string
	}{
		{"flags first", []string{"--limit", "3", "query"}, []string{"--limit", "3", "--", "query"}},
		{"trailing flags move ahead of query", []string{"query", "words", "--limit", "3", "--hidden"}, []string{"--limit", "3", "--hidden", "--", "query", "words"}},
		{"equals form keeps its value", []string{"q", "--limit=4"}, []string{"--limit=4", "--", "q"}},
		{"double dash keeps dashes in query", []string{"--hidden", "--", "--limit", "query"}, []string{"--hidden", "--", "--limit", "query"}},
		{"no positional adds no separator", []string{"--limit", "2"}, []string{"--limit", "2"}},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := ReorderInterspersed(fs, tt.args); !reflect.DeepEqual(got, tt.want) {
				t.Errorf("ReorderInterspersed(%v) = %v, want %v", tt.args, got, tt.want)
			}
		})
	}
}
