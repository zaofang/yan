#include <stdint.h>
#include <stddef.h>

typedef struct Str Str;
typedef struct List List;

extern Str *yan_rt_show_i64(int64_t);
extern Str *yan_rt_concat(const Str *, const Str *);
extern uint8_t yan_rt_str_eq(const Str *, const Str *);
extern void yan_rt_print_str(const Str *);
extern List *yan_rt_list_new(uint64_t words);
extern uint64_t *yan_rt_list_push(List *);
extern int64_t yan_rt_list_len(const List *);
extern uint64_t *yan_rt_list_at(List *, int64_t);
extern void yan_rt_exit(int64_t);

int main(void) {
    Str *a = yan_rt_show_i64(12);
    Str *b = yan_rt_show_i64(34);
    Str *ab = yan_rt_concat(a, b);
    if (!yan_rt_str_eq(ab, yan_rt_concat(a, b))) yan_rt_exit(2);
    if (yan_rt_str_eq(ab, a)) yan_rt_exit(3);
    yan_rt_print_str(ab);

    List *l = yan_rt_list_new(1);
    for (uint64_t i = 0; i < 5; i++) *yan_rt_list_push(l) = i * i;
    if (yan_rt_list_len(l) != 5) yan_rt_exit(4);
    if (*yan_rt_list_at(l, 4) != 16) yan_rt_exit(5);
    yan_rt_exit(0);
}
