let count1 = 0
let count2 = 0

window.plus1_1 = function() {
    count1 += 1
    document.getElementById("count1").textContent = count1
}

window.plus2_1 = function() {
    count1 += 2
    document.getElementById("count1").textContent = count1
}

window.plus3_1 = function() {
    count1 += 3
    document.getElementById("count1").textContent = count1
}

window.plus1_2 = function() {
    count2 += 1
    document.getElementById("count2").textContent = count2
}

window.plus2_2 = function() {
    count2 += 2
    document.getElementById("count2").textContent = count2
}

window.plus3_2 = function() {
    count2 += 3
    document.getElementById("count2").textContent = count2
}

window.reset = function() {
    count1 = 0
    count2 = 0
    document.getElementById("count1").textContent = 0
    document.getElementById("count2").textContent = 0
}