        section code_f,code_f

code_start
        moveq   #0,d0
;        lea     data_middle,a0
        rts
code_middle
        nop
code_end

        section data,data

        dc.l    $12345678
data_middle
        dc.l    $87654321

        section data_c,data_c

        dc.l    $01264444
        dc.l    $00120024
        dc.l    $23212344
        dc.l    $44332211
        ds.l    2

        section bss,bss

bss_start
        ds.b    12
bss_end
