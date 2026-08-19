

function plus1_1(){
    count1 = Number(document.getElementById("count1").textContent);
    count1 +=1
    document.getElementById("count1").textContent = count1
}
function plus2_1(){
    count1 = Number(document.getElementById("count1").textContent);
    count1 +=2
    document.getElementById("count1").textContent = count1
}
function plus3_1(){
    count1 = Number(document.getElementById("count1").textContent);
    count1 +=3
    document.getElementById("count1").textContent = count1
}
function plus1_2(){
    count2 = Number(document.getElementById("count2").textContent);
    count2++
    document.getElementById("count2").textContent = count2
}
function plus2_2(){
    count2 = Number(document.getElementById("count2").textContent);
    count2+=2
    document.getElementById("count2").textContent = count2
}
function plus3_2(){
    count2 = Number(document.getElementById("count2").textContent);
    count2+=3
    document.getElementById("count2").textContent = count2
}
function reset(){
    document.getElementById("count1").textContent=0
    document.getElementById("count2").textContent=0

}