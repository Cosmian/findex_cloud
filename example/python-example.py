from cloudproof_py.findex import FindexCloud, Label, Location

token = "BHExm/EoaVE4LZI/v1qmJPeXG1QAkd+zC2nX49Tohg/E8fjc5AaffXQUoM0XELADguZACi4QC5ucy7Mw3z5VHYExqcWcOkwOUOAsH0Z8eVfJmX6Y3ESxE";
label = Label.from_string("Hello World!")


FindexCloud.upsert({
    Location.from_string("1"): ["John", "Doe"],
    Location.from_string("2"): ["Jane", "Doe"],
  },
  token,
  label,
  base_url="http://localhost:8080"
)


results = FindexCloud.search(["Doe"], token, label, base_url="http://localhost:8080"
)

print(results)
