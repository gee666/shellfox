(
    shellfox_status=$?
    LC_ALL=C
    if shellfox_cwd=$(builtin pwd -P && printf .); then
        shellfox_cwd=${shellfox_cwd%$'\n.'}
        shellfox_uri=file://
        for ((shellfox_i=0; shellfox_i<${#shellfox_cwd}; shellfox_i++)); do
            shellfox_byte=${shellfox_cwd:shellfox_i:1}
            case $shellfox_byte in
                [a-zA-Z0-9._~/-]) shellfox_uri+=$shellfox_byte ;;
                *) printf -v shellfox_hex '%%%02X' "'$shellfox_byte"; shellfox_uri+=$shellfox_hex ;;
            esac
        done
        printf '\033]7;%s\007' "$shellfox_uri"
    else
        printf '\033]7;\007'
    fi
    exit "$shellfox_status"
)
